# Bedrock AgentCore Runtime のコンテナ契約（linux/arm64、0.0.0.0:8080、GET /ping・POST /invocations）
# に合わせたイメージ。arm64 の指定は cdk 側の AgentRuntimeArtifact（platform: LINUX_ARM64）
# またはローカルでの `docker build --platform linux/arm64` で行う。

FROM rust:1.97-bookworm AS build
WORKDIR /app

# 依存クレートだけを先にビルドして、ソース変更時のビルドでキャッシュを効かせる。
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked \
    && rm -r src

COPY src ./src
RUN touch src/main.rs && cargo build --release --locked

# TLS は rustls なので libssl は不要。CA 証明書は distroless に含まれている。
FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /app/target/release/agentcore-rust-agent /usr/local/bin/agentcore-rust-agent
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/agentcore-rust-agent"]
