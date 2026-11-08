# AgentCore Runtime で Rust のエージェントを動かせるか（2026-11-08 時点）

## 結論
- **動かせる。ただしコンテナでのデプロイに限る。**
  - Runtime はどの言語で書いたかを問わない。条件は、コンテナが次の 3 つを満たすこと。
    - 決められたポートとパスで HTTP を受け付ける
    - ARM64 向けにビルドされている
    - ECR に置かれている
  - ZIP をそのままデプロイする方式（direct code deployment）は使えない。対応言語は Python 3.10〜3.14 と Node.js 22 だけ。
- **Rust 向けの公式エージェント SDK やフレームワークは見つからなかった（未確認）。**
  - Runtime 用の SDK（`bedrock-agentcore` の `BedrockAgentCoreApp`）は Python 向けのみ。
  - Strands Agents も Python 版と TypeScript 版だけ。
- **AWS SDK for Rust には AgentCore 用のクレートがある。**
  - `aws-sdk-bedrockagentcore`（データプレーン）: Runtime の呼び出し、Memory、Browser / Code Interpreter の操作など
  - `aws-sdk-bedrockagentcorecontrol`（コントロールプレーン）: Runtime の作成・管理など

## Runtime がコンテナに求める条件
| プロトコル | ポート | パス |
|---|---|---|
| HTTP | 8080 | `POST /invocations`、`GET /ping`（どちらも必須） |
| WebSocket | 8080 | `/ws` |
| MCP | 8000 | `/mcp` |
| A2A | 9000 | `/` |

- `0.0.0.0` で待ち受け、`linux/arm64` 向けにビルドする。
- `/ping` は `{"status":"Healthy"}` または `{"status":"HealthyBusy"}` を返す。
- ポートと呼び出しパスは `protocolConfiguration` で変更できる。指定できるポートは 8000〜12000。
- 次のポートは予約されていて使えない: 1144、1514、1571、2049、4317-4318、8443、48620、48640、53530-53532。

## Rust での実装方針
1. axum などの Web フレームワークで `POST /invocations` と `GET /ping` を実装する。
2. Bedrock のモデルは `aws-sdk-bedrockruntime` の Converse / ConverseStream で呼ぶ。エージェントのループ処理は自分で書くか、`rig` などのクレートを使う。
3. `aarch64-unknown-linux-gnu`（または musl）向けにクロスビルドし、ECR に push する。
4. Runtime を作成する。CDK の `aws_bedrockagentcore.Runtime` を使うか、`create_agent_runtime` の `containerConfiguration` にイメージを指定する。

## 注意点（Python SDK が自動でやってくれることを自前で実装する）
- **セッション ID**: リクエストの `X-Amzn-Bedrock-AgentCore-Runtime-Session-Id` ヘッダから自分で取り出す。
- **任意のヘッダ**: エージェントに渡すには、`requestHeaderConfiguration` で許可リストに登録する必要がある。
  - `x-amz-*` と `x-amzn-*` で始まるヘッダは登録できない。
  - 例外として、`X-Amzn-Bedrock-AgentCore-Runtime-Custom-` で始まるものは登録できる。
- **ストリーミング**: `text/event-stream` の SSE（`data: {...}` 形式）で返す。axum の `Sse` で実装できる。
- **Observability（OTEL）**: Python 用の ADOT 自動計装は Rust では使えない。
  - Rust では `opentelemetry` と OTLP の組み込みを自分で行う。
  - OTEL を入れなくてもエージェントは動き、基本的なランタイムメトリクスはサービス側で取得される。
  - サービス側のスパンを見るには、CloudWatch Transaction Search を有効にする。
- **Gateway**: MCP のエンドポイントなので、Rust の MCP クライアント（`rmcp` など）か、HTTP で JSON-RPC を直接送って呼ぶ。具体的な Rust のサンプルは見つからなかった（未確認）。
- **AgentCore CLI**（`@aws/agentcore`）が Rust のひな形作成やビルドに対応しているかは未確認。

## 実例
- AWS Community Builders の記事 "Deploy Rust agent to AWS AgentCore Runtime with GitHub Actions"（2025-11）
  - 構成: axum + `rig` + `aws-sdk-bedrockruntime`。aarch64 向けにクロスビルドし、イメージは約 40MB。CDK でデプロイしている。
  - 扱っていない内容: ストリーミング、セッションヘッダ、OTEL。
- Go や Java の公式サンプルは見つからなかった（未確認）。

## 参考
- https://docs.aws.amazon.com/bedrock-agentcore/latest/devguide/runtime-service-contract.html
- https://docs.aws.amazon.com/bedrock-agentcore/latest/devguide/runtime-http-protocol-contract.html
- https://docs.aws.amazon.com/bedrock-agentcore/latest/devguide/getting-started-custom.html
- https://docs.aws.amazon.com/bedrock-agentcore/latest/devguide/runtime-code-deploy-supported-runtimes.html
- https://docs.aws.amazon.com/bedrock-agentcore/latest/devguide/runtime-header-allowlist.html
- https://docs.aws.amazon.com/bedrock-agentcore/latest/devguide/observability-configure.html
- https://docs.rs/aws-sdk-bedrockagentcore
- https://docs.rs/aws-sdk-bedrockagentcorecontrol
- https://dev.to/aws-builders/deploy-rust-agent-to-aws-agentcore-runtime-with-github-actions-3gf8
