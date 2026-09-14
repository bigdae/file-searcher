# FileSearcher — 개발/빌드/테스트 자동화
.PHONY: check test fmt clippy build dev frontend clean help

FRONTEND_DIR = .
TAURI_DIR     = src-tauri

.DEFAULT_GOAL := help

help:
	@echo "FileSearcher — 사용 가능한 타깃"
	@echo ""
	@echo "  make dev      개발 모드 실행 (vite + Tauri 창)"
	@echo "  make build    배포용 설치 파일 생성 (.dmg / .exe / .msi)"
	@echo "  make test     통합 테스트 (한글 검색, 증분 업데이트)"
	@echo "  make check    컴파일 체크"
	@echo "  make fmt      코드 포맷"
	@echo "  make clippy   린트"
	@echo "  make frontend 프론트엔드만 빌드 (vite)"
	@echo "  make clean    빌드 산출물 제거"


## 기본: 컴파일 체크
check:
	cargo check --manifest-path $(TAURI_DIR)/Cargo.toml

## 통합 테스트 (한글 검색, 증분 업데이트)
test:
	cargo test --manifest-path $(TAURI_DIR)/Cargo.toml

## 코드 포맷
fmt:
	cargo fmt --manifest-path $(TAURI_DIR)/Cargo.toml

## 린트
clippy:
	cargo clippy --manifest-path $(TAURI_DIR)/Cargo.toml

## 프론트엔드만 빌드 (vite)
frontend:
	npm install
	npm run build

## 개발 모드 (vite dev + Tauri 창)
dev:
	cargo tauri dev

## 배포용 설치 파일 생성 (macOS: .dmg / Windows: .exe, .msi)
build: frontend
	cargo tauri build

## 빌드 산출물 제거
clean:
	rm -rf dist $(TAURI_DIR)/target
