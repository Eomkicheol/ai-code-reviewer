FROM rust:1.87-slim AS builder

WORKDIR /app

# 의존성 캐시 레이어 분리 (소스 변경 시 재빌드 최소화)
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main(){}" > src/main.rs && echo "" > src/lib.rs
RUN cargo build --release
RUN rm -rf src

# 실제 소스 빌드
COPY src ./src
RUN touch src/main.rs src/lib.rs && cargo build --release

# 런타임 이미지 (최소화)
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/reviewer /usr/local/bin/reviewer

EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:3000/health || exit 1

CMD ["reviewer"]
