# Windows·macOS 파일 검색 프로그램 개발 계획서

## 1. 프로젝트 개요

macOS에서 개발하고 Windows와 macOS에 배포할 수 있는 로컬 파일 검색 프로그램을 만든다.

사용자가 검색할 폴더를 지정하면 해당 폴더의 파일명, 경로, 문서 내용을 미리 추출해 검색 인덱스를 생성한다. 이후 사용자가 검색어를 입력하면 원본 파일을 매번 읽지 않고 인덱스에서 즉시 결과를 찾는다.

### 핵심 목표

- 사용자가 검색 대상 폴더를 직접 추가·삭제할 수 있다.
- 파일명, 경로, 문서 내용으로 검색할 수 있다.
- `.doc`, `.docx`, `.ppt`, `.pptx`, `.xls`, `.xlsx`, `.pdf`의 내용 검색을 지원한다.
- `.txt`, `.md`, `.csv`, `.json`, `.xml`, `.html` 같은 텍스트 파일도 검색한다.
- 최초 전체 인덱싱 후에는 변경된 파일만 증분 업데이트한다.
- Windows와 macOS에서 같은 기능과 유사한 UI를 제공한다.
- 검색 결과에 검색어 주변 문장과 파일 정보를 표시한다.

> HWP와 HWPX는 초기 개발 범위에서 제외한다.

---

## 2. 전체 동작 흐름

```text
사용자
  ↓
검색할 폴더 추가
  ↓
폴더 스캔
  ↓
파일 종류 판별
  ↓
문서별 텍스트 추출
  ↓
검색 인덱스 생성
  ↓
파일 변경 감시
  ↓
인덱스 증분 업데이트

사용자 검색
  ↓
Tantivy 인덱스 조회
  ↓
검색 결과와 문맥 표시
```

이 프로그램은 검색할 때마다 모든 파일을 읽는 방식이 아니다. 폴더를 등록할 때 문서 내용을 미리 읽어 검색용 데이터베이스를 만들고, 실제 검색은 그 인덱스를 대상으로 수행한다.

예를 들어 다음 폴더를 등록할 수 있다.

```text
/Users/me/Documents
D:\회사자료
D:\Projects
```

처음 한 번은 전체 파일을 인덱싱하고, 이후에는 새로 생성되거나 수정·이동·삭제된 파일만 반영한다.

---

## 3. 추천 기술 스택

| 영역 | 추천 기술 | 역할 |
|---|---|---|
| 화면 | React + TypeScript | 검색·설정 UI |
| 데스크톱 앱 | Tauri 2 | Windows/macOS 앱 패키징 및 네이티브 연동 |
| 핵심 로직 | Rust | 스캔, 추출, 인덱싱, 검색 |
| 전문 검색 | Tantivy | 로컬 전문 검색 인덱스 |
| 관리 DB | SQLite | 폴더 및 파일 상태 관리 |
| 변경 감지 | Rust `notify` | 파일 생성·수정·이동·삭제 감지 |
| 문서 내용 추출 | Apache Tika + Rust 직접 처리 | Office, PDF, 텍스트 추출 |
| 비동기 처리 | Tokio | 작업 큐와 병렬 처리 |
| Windows 배포 | NSIS 또는 MSI | 설치 프로그램 |
| macOS 배포 | DMG | 설치 이미지 |
| 자동 빌드 | GitHub Actions | OS별 빌드 및 산출물 생성 |

### 추천 조합

```text
Tauri 2
 + React/TypeScript
 + Rust
 + Tantivy
 + SQLite
 + Apache Tika
 + File Watcher
```

Tauri는 하나의 프런트엔드 코드베이스로 Windows와 macOS 데스크톱 앱을 만들 수 있다. Rust로 파일 시스템과 검색 엔진을 다루기 때문에 로컬 인덱싱 프로그램에도 잘 맞는다.

Tantivy는 Rust 기반 전문 검색 엔진이다. 별도의 Elasticsearch 서버 없이 앱 내부에서 빠른 검색과 증분 인덱싱을 구현할 수 있다.

---

## 4. 지원 파일 형식

### 1차 지원 대상

| 구분 | 확장자 | 검색 범위 |
|---|---|---|
| Word | `.doc`, `.docx` | 파일명, 경로, 본문 |
| PowerPoint | `.ppt`, `.pptx` | 파일명, 경로, 슬라이드 텍스트 |
| Excel | `.xls`, `.xlsx` | 파일명, 경로, 셀 텍스트 |
| PDF | `.pdf` | 파일명, 경로, 추출 가능한 본문 |
| 텍스트 | `.txt`, `.md`, `.csv` | 전체 텍스트 |
| 개발·데이터 | `.json`, `.xml`, `.html` | 전체 텍스트 |

### 선택 지원 대상

```text
.rtf
.odt
.ods
.odp
```

### 이미지 파일

초기 버전에서는 `.jpg`, `.png`, `.gif`, `.webp` 등을 파일명과 경로로만 검색한다. 이미지 속 글자를 찾는 OCR은 후속 기능으로 분리한다.

---

## 5. 문서 내용 추출 전략

최신 Office 형식인 `.docx`, `.pptx`, `.xlsx`는 ZIP과 XML 기반이다. 반면 구형 `.doc`, `.ppt`, `.xls`는 OLE2 바이너리 형식이므로 직접 파서를 만드는 비용이 크다.

따라서 파일 종류에 따라 추출기를 분리한다.

```text
File
 ↓
Extractor Router
 ├─ txt/md/csv/json/xml/html → Rust에서 직접 읽기
 ├─ doc/docx                → Apache Tika
 ├─ ppt/pptx                → Apache Tika
 ├─ xls/xlsx                → Apache Tika
 └─ pdf                     → Apache Tika
 ↓
Plain Text + Metadata
```

Apache Tika는 구형·신형 Microsoft Office 파일과 PDF 등 여러 문서 형식에서 텍스트와 메타데이터를 추출할 수 있다.

예를 들어 `회의자료.pptx` 안에 다음 문장이 있다면,

```text
2027년 스마트팩토리 구축 계획
```

사용자가 `스마트팩토리`를 검색했을 때 해당 PPT 파일이 결과에 표시되어야 한다.

### 추출기 공통 인터페이스 예시

```rust
trait ContentExtractor {
    fn supports(&self, path: &Path) -> bool;

    fn extract(&self, path: &Path)
        -> Result<ExtractedDocument>;
}
```

```rust
struct ExtractedDocument {
    title: Option<String>,
    content: String,
    author: Option<String>,
    created_at: Option<DateTime>,
}
```

모든 추출기가 동일한 `ExtractedDocument` 형태를 반환하도록 하면 나중에 추출 방식을 바꾸거나 새 파일 형식을 추가하기 쉽다.

---

## 6. 프로젝트 구조

```text
file-search-app/
├─ src/                         # React UI
│  ├─ pages/
│  │  ├─ SearchPage.tsx
│  │  └─ SettingsPage.tsx
│  ├─ components/
│  │  ├─ SearchBox.tsx
│  │  ├─ SearchResult.tsx
│  │  ├─ FolderList.tsx
│  │  └─ IndexStatus.tsx
│  └─ api/
│     └─ search.ts
├─ src-tauri/
│  └─ src/
│     ├─ main.rs
│     ├─ search/
│     │  ├─ mod.rs
│     │  ├─ index.rs
│     │  └─ query.rs
│     ├─ scanner/
│     │  ├─ scanner.rs
│     │  └─ watcher.rs
│     ├─ extractor/
│     │  ├─ mod.rs
│     │  ├─ text.rs
│     │  ├─ office.rs
│     │  └─ pdf.rs
│     ├─ database/
│     │  └─ sqlite.rs
│     └─ commands/
│        ├─ search.rs
│        ├─ folder.rs
│        └─ index.rs
└─ resources/
   └─ tika/
```

핵심은 다음 계층을 분리하는 것이다.

```text
Scanner
  ↓
Extractor
  ↓
Document
  ↓
Indexer
  ↓
Search Engine
```

이렇게 하면 Apache Tika를 다른 추출기로 바꾸더라도 폴더 스캔이나 검색 엔진을 크게 수정하지 않아도 된다.

---

## 7. Tantivy 인덱스 설계

### 주요 필드

```text
id
path
filename
extension
content
modified_at
size
folder_id
```

검색 대상은 기본적으로 `filename`, `path`, `content`이다.

### 권장 가중치

```text
filename × 5
path     × 2
content  × 1
```

예를 들어 `회의록`을 검색하면 `회의록_2026.docx`처럼 파일명에 검색어가 포함된 문서를 본문에만 검색어가 있는 문서보다 위에 표시한다.

### 검색 결과 문맥

검색 결과에는 파일명과 경로뿐 아니라 검색어 주변의 문장을 표시한다.

```text
사업계획서.docx
D:\Documents\2026\사업계획서.docx

...당사의 스마트팩토리 구축 계획은 2027년부터...

DOCX · 2.3 MB · 2026-09-12
```

사용자는 이 문맥을 통해 파일을 열기 전에 자신이 찾던 자료인지 판단할 수 있다.

---

## 8. SQLite와 Tantivy의 역할

### SQLite

폴더와 파일의 관리 상태를 저장한다.

`folders` 예시:

```text
id
path
enabled
created_at
last_indexed_at
```

`files` 예시:

```text
id
path
size
mtime
hash
indexed_at
status
```

### Tantivy

검색에 필요한 필드를 저장하고 전문 검색을 수행한다.

```text
filename
path
content
extension
modified_at
```

정리하면 다음과 같다.

```text
SQLite = 폴더와 파일 상태를 관리하는 DB
Tantivy = 실제 검색을 수행하는 검색 인덱스
```

---

## 9. 최초 인덱싱

사용자가 `D:\회사자료`를 등록하면 다음 순서로 처리한다.

```text
Directory Scanner
      ↓
File Filter
      ↓
Metadata Check
      ↓
Extractor Queue
      ↓
Tika / Text Parser
      ↓
Index Writer
      ↓
Tantivy
```

### 병렬 처리

문서 추출은 여러 worker가 병렬로 처리한다.

```text
worker 1 → docx
worker 2 → pdf
worker 3 → ppt
worker 4 → txt
```

초기 기본값은 worker 4개 정도로 시작하고, CPU와 디스크 사용량을 측정해 조정한다. 무제한 병렬 처리는 디스크 I/O와 메모리를 과도하게 사용할 수 있으므로 작업 큐의 동시 실행 수를 제한해야 한다.

### 변경 여부 판단

모든 파일에 매번 SHA-256을 계산하면 큰 파일이 많을 때 느려진다. 우선 다음 값을 비교한다.

```text
path
file size
modified time
```

셋 중 하나가 달라졌을 때만 재추출·재인덱싱하고, 충돌 가능성을 더 줄여야 하는 특정 상황에서만 해시를 계산한다.

---

## 10. 파일 변경 감지와 증분 업데이트

파일 하나가 바뀔 때 전체 인덱스를 다시 만들면 안 된다.

```text
File Watcher
      ↓
create / modify / rename / delete
      ↓
SQLite 상태 갱신
      ↓
기존 검색 문서 삭제
      ↓
필요한 파일만 다시 추출
      ↓
Tantivy에 새 문서 추가
```

처리해야 하는 주요 이벤트:

- 파일 생성
- 파일 수정
- 파일명 또는 경로 변경
- 파일 삭제
- 폴더 이동 또는 삭제
- 앱이 꺼져 있는 동안 발생한 변경

앱이 다시 시작될 때 파일 메타데이터를 비교해 감시하지 못한 변경도 보정해야 한다.

---

## 11. 폴더 관리와 제외 규칙

설정 화면에서 검색 위치를 관리한다.

```text
검색 위치

✓ ~/Documents
  12,341 files
  마지막 인덱싱: 2분 전

✓ ~/Projects
  52,811 files
  마지막 인덱싱: 방금

+ 폴더 추가
```

폴더별 기능:

- 인덱싱 시작 또는 일시정지
- 검색 범위에서 활성화 또는 비활성화
- 폴더 제거
- 전체 재인덱싱
- 오류 파일 목록 확인

### 기본 제외 패턴

```text
node_modules
.git
target
dist
build
.cache
~$*
.DS_Store
```

사용자가 개발 폴더를 등록했을 때 `node_modules`나 빌드 결과물까지 읽으면 파일 수가 급격히 증가하므로 기본 제외 규칙이 필요하다. 사용자가 규칙을 추가·해제할 수 있게 한다.

---

## 12. 검색 기능

### 일반 검색

```text
스마트팩토리
```

### 여러 단어 검색

```text
스마트팩토리 구축
```

### 정확한 문구 검색

```text
"스마트팩토리 구축 계획"
```

### 파일명 검색

```text
name:보고서
```

### 확장자 필터

```text
ext:pdf
```

### 경로 필터

```text
path:project
```

### 조건 조합

```text
스마트팩토리 ext:pptx
```

### 정렬 방식

- 관련도순
- 최신 수정순
- 파일명순
- 파일 크기순

---

## 13. 한글 검색 고려사항

한글 토큰화는 초기 프로토타입에서 반드시 검증해야 한다.

```text
스마트팩토리
스마트 팩토리
스마트공장
```

최소 테스트 대상:

- 붙여 쓴 한글과 띄어 쓴 한글
- 조사 포함 단어
- 한글과 영문 혼합어
- 숫자가 포함된 파일명
- 특수문자가 포함된 경로
- 대소문자가 다른 영문

향후 동의어 사전을 추가할 수 있다.

```text
AI = 인공지능
MES = 생산관리시스템
스마트팩토리 = 스마트공장
```

초기에는 정확한 키워드 검색의 안정성을 먼저 확보하고, 형태소 분석이나 고급 동의어 처리는 후속 단계로 진행한다.

---

## 14. UI 구성

### 검색 화면

```text
┌─────────────────────────────────────────────┐
│ 🔍 스마트팩토리 구축                       │
└─────────────────────────────────────────────┘

검색 범위: [전체] [문서] [PDF] [PPT] [Excel]
정렬: 관련도 ▼
```

결과 항목에 표시할 정보:

- 파일명
- 전체 경로
- 검색어 주변 문맥
- 확장자
- 파일 크기
- 최종 수정일
- 검색어 하이라이트
- 파일 열기 및 폴더에서 보기

### 설정 화면

- 검색 폴더 목록
- 폴더 추가·삭제
- 제외 규칙
- 파일 형식별 검색 여부
- 인덱싱 상태와 진행률
- 마지막 인덱싱 시각
- 오류 및 재시도
- 인덱스 전체 재생성

---

## 15. macOS에서 개발하고 Windows로 배포하기

React, Tauri, Rust, Tantivy 기반 코드는 macOS에서 개발할 수 있다.

최종 설치 파일은 GitHub Actions에서 운영체제별 runner를 사용해 빌드하는 방식을 권장한다.

```text
Mac에서 개발
      ↓
Git Push
      ↓
GitHub Actions
      ├─ macos-latest
      │      ↓
      │     DMG
      └─ windows-latest
             ↓
          EXE / MSI
```

Windows 설치 파일은 Windows runner에서, macOS DMG는 macOS runner에서 생성한다. 특히 MSI는 Windows 환경에서 빌드하는 것이 안전하다.

출시 단계에서는 Windows 코드 서명과 macOS Developer ID 서명·공증도 준비해야 한다.

---

## 16. 개발 단계

### Phase 1 — 검색 엔진 프로토타입

UI 없이 CLI로 먼저 만든다.

```text
index ~/Documents
search "스마트팩토리"
```

초기 지원 파일:

```text
txt
md
```

검증 항목:

- 폴더 스캔 속도
- 인덱스 생성 속도
- 인덱스 크기
- 한글 검색 정확도
- 1만·10만 파일 검색 응답 속도

### Phase 2 — Tauri UI

React 화면을 연결한다.

- 폴더 선택
- 인덱싱 시작·중지
- 검색
- 결과 표시
- 파일 열기
- 파일이 있는 폴더 열기

### Phase 3 — Office와 PDF 내용 검색

Apache Tika 연동 후 다음 형식을 추가한다.

```text
doc
docx
ppt
pptx
xls
xlsx
pdf
```

손상된 문서, 암호화 문서, 텍스트가 없는 스캔 PDF 등도 오류 처리해야 한다.

### Phase 4 — 파일 변경 감지

다음 이벤트를 증분 반영한다.

```text
create
modify
rename
delete
```

### Phase 5 — 검색 UX 개선

- 검색어 하이라이트
- 문맥 snippet
- 파일 형식 필터
- 날짜·크기 필터
- 정렬
- 최근 검색어
- 검색 기록
- 검색 중 키보드 탐색

### Phase 6 — 배포

Windows:

```text
MySearchSetup.exe
MySearch.msi
```

macOS:

```text
MySearch.dmg
```

추후 자동 업데이트, 코드 서명, macOS 공증을 추가한다.

---

## 17. MVP 범위

### 포함

- 검색 폴더 추가·삭제
- TXT·MD 내용 검색
- DOC·DOCX·PPT·PPTX·XLS·XLSX·PDF 내용 검색
- 파일명·경로·본문 검색
- Tantivy 인덱스
- SQLite 상태 관리
- 파일 변경 감지
- 검색 결과 문맥과 하이라이트
- 파일 열기와 폴더에서 보기
- Windows와 macOS 설치 파일

### 초기 버전에서 제외

- HWP·HWPX
- 이미지 OCR
- AI 자연어 검색
- 의미 기반 벡터 검색
- NAS와 네트워크 드라이브
- 클라우드 저장소 검색
- 문서 전체 미리보기
- 모바일 앱

MVP의 핵심 성공 기준은 다음과 같다.

> Windows에서 등록한 폴더에 파일이 10만 개 있어도 검색 결과가 즉각적으로 표시되는가?

---

## 18. 성능 및 안정성 목표

초기 목표 예시:

| 항목 | 목표 |
|---|---|
| 일반 검색 응답 | 100ms 이내 목표 |
| 검색 결과 첫 화면 | 200ms 이내 목표 |
| 동시 문서 추출 worker | 기본 4개 |
| 대용량 파일 제한 | 설정 가능 |
| 오류 파일 처리 | 전체 작업을 중단하지 않고 기록 후 계속 |
| 변경 이벤트 | 짧은 시간 동안 모아 중복 처리 방지 |
| 앱 재시작 | 중단 지점과 파일 상태를 기반으로 재개 |

정확한 수치는 테스트 PC와 문서 구성에 따라 조정한다.

---

## 19. 주요 위험 요소와 대응

| 위험 | 대응 방안 |
|---|---|
| Tika 실행을 위해 Java가 필요함 | 앱에 런타임 포함 여부 검토 또는 별도 경량 추출기로 단계적 교체 |
| 손상·암호화 문서 | 타임아웃, 오류 기록, 개별 파일 건너뛰기 |
| 대용량 PDF·Excel | 파일 크기 제한, 추출 시간 제한, 작업 취소 |
| 파일 변경 이벤트 중복 | debounce와 작업 큐 중복 제거 |
| 한글 검색 품질 | 초기 토큰화 테스트와 회귀 테스트 구축 |
| 인덱스와 실제 파일 불일치 | 시작 시 메타데이터 재검증과 주기적 보정 스캔 |
| Windows/macOS 경로 차이 | OS 독립 경로 계층과 플랫폼별 테스트 |
| 설치 파일 경고 | 코드 서명과 macOS 공증 적용 |

---

## 20. 테스트 계획

### 기능 테스트

- 폴더 추가·삭제
- 중복 폴더 등록
- 상위·하위 폴더 중복 등록
- 파일 생성·수정·이름 변경·삭제
- 앱 종료 중 발생한 변경 반영
- 파일 열기와 폴더 열기

### 문서 추출 테스트

- 정상 문서
- 빈 문서
- 암호화 문서
- 손상 문서
- 매우 큰 문서
- 한글·영문·숫자 혼합 문서
- 표와 슬라이드가 많은 문서
- 텍스트가 없는 스캔 PDF

### 성능 테스트

- 1천 파일
- 1만 파일
- 10만 파일
- 대용량 PDF가 많은 폴더
- 작은 텍스트 파일이 매우 많은 폴더
- 인덱싱 중 검색
- 대량 변경 이벤트 발생

### 플랫폼 테스트

- Windows 10/11
- macOS 최신 버전과 최소 지원 버전
- 한글 경로
- 긴 경로
- 접근 권한이 없는 폴더
- 외장 디스크 연결·해제

---

## 21. 향후 확장 기능

기본 키워드 검색이 안정화되면 다음 기능을 추가할 수 있다.

- 이미지 및 스캔 PDF OCR
- 자연어 검색
- 문서 요약
- 태그와 즐겨찾기
- 중복 파일 탐지
- NAS와 네트워크 드라이브
- OneDrive·Google Drive 등 클라우드 연동
- 검색 결과 미리보기
- 유사 문서 검색

AI 검색은 기존 키워드 검색을 없애는 방식보다 Tantivy 검색과 벡터 검색을 결합하는 하이브리드 방식이 적합하다.

```text
사용자 질문
   ↓
키워드 검색 + 벡터 검색
   ↓
결과 통합 및 재정렬
```

처음부터 벡터 DB로 시작하지 않고, 정확한 파일명·키워드 검색을 먼저 완성한다.

---

## 22. 최종 권장 방향

이 프로젝트의 권장 구조는 다음과 같다.

```text
React UI
   ↓ Tauri IPC
Rust Core
   ├─ Folder Scanner
   ├─ File Watcher
   ├─ Extractor Router
   │    ├─ Text Parser
   │    └─ Apache Tika
   ├─ SQLite
   └─ Tantivy
```

가장 먼저 구현할 부분은 React 화면이 아니라 다음 흐름이다.

```text
Scanner → Extractor → Tantivy → Search
```

CLI 프로토타입에서 TXT와 MD 파일로 인덱싱 및 한글 검색 성능을 확인한 뒤 Tauri UI와 Office/PDF 추출을 단계적으로 추가하는 것이 가장 안전하다.

---

## 참고 자료

- [Tauri](https://tauri.app/)
- [Tauri 2 GitHub Actions 배포 가이드](https://v2.tauri.app/distribute/pipelines/github/)
- [Tauri Windows Installer 안내](https://v2.tauri.app/distribute/windows-installer/)
- [Tantivy GitHub 저장소](https://github.com/quickwit-oss/tantivy)
- [Apache Tika 지원 형식](https://tika.apache.org/3.0.0/formats.html)

