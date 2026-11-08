# Rust Agent on Amazon Bedrock AgentCore Runtime

Rust で書いたエージェントを Amazon Bedrock AgentCore Runtime で動かすサンプルです。

- **エージェント（`agent/`）**
  - axum で Runtime のコンテナ契約（`0.0.0.0:8080`、`GET /ping`、`POST /invocations`）を実装している。
  - `aws-sdk-bedrockruntime` の ConverseStream で、ツール呼び出しのループを自前で書いている。
  - 応答は AG-UI プロトコルのイベントとして SSE で返す。
- **フロント（`frontend/`）**
  - Next.js と CopilotKit。[clouddev-code/aws-cost-visual-report](https://github.com/clouddev-code/aws-cost-visual-report) の frontend を流用している。
  - ツールの結果は Recharts のグラフで表示する。
- **IaC（`cdk/`）**
  - Runtime（`protocolConfiguration: AGUI`）と、フロントを動かす Lambda（Lambda Web Adapter と Function URL）。

構成を検討したときの資料は `AgentCore-Runtime-Rust実装可否-20261108.md` と `Rust版AgentCoreサンプル-構成検討-20261108.md` にあります。

```
Browser ─→ Lambda Function URL (RESPONSE_STREAM, AuthType=NONE)
             └ Next.js /api/copilotkit（CopilotRuntime）
                  └ InvokeAgentRuntime（SigV4）
                       └ AgentCore Runtime（AGUI）→ Rust コンテナ POST /invocations（SSE）
                            └ Bedrock ConverseStream（us.anthropic.claude-haiku-4-5-20251001-v1:0）
```

## 前提
- Rust 1.94.1 以上（`agent/rust-toolchain.toml` は 1.97 を指定）
- Node.js 22
- Docker（OrbStack など）。コンテナは `linux/arm64` でビルドする。
- us-west-2 で Claude Haiku 4.5 を呼び出せる AWS 認証情報

## エージェントの構成（`agent/src/`）
| ファイル | 役割 |
|---|---|
| `main.rs` | axum のルーター。`/ping` は処理中の run があれば `HealthyBusy`、なければ `Healthy` を返す。`/invocations` は run を別タスクで動かし、10 秒ごとのハートビート付き SSE で返す |
| `agui.rs` | AG-UI の `RunAgentInput` とイベントの型（camelCase） |
| `agent_loop.rs` | ConverseStream のループ。テキスト差分は `TEXT_MESSAGE_CONTENT`、ツール入力の差分は `TOOL_CALL_ARGS` として、届いたそばから送る。`stopReason=tool_use` の間はツールを実行して再度呼ぶ（上限 8 回） |
| `convert.rs` | AG-UI のメッセージ履歴を Converse 形式に変換する。同じ role が続いたらまとめ、対応する結果がない toolUse と、対応する呼び出しがない toolResult を捨てる |
| `tools.rs` | `Tool` トレイトと、デモ用のツール `get_sample_metrics`（ダミーのメトリクス）と `get_current_time` |

ストリームを始めたあとにエラーが起きた場合は、HTTP 200 のまま `RUN_ERROR` を送って終わります。

## ローカルで動かす

エージェントを起動します。認証情報は aws-config の標準の読み込み順で解決されます（`aws login` のセッションにも対応しています）。

```fish
cd agent
set -x AWS_REGION us-west-2
cargo run
```

動作を確認します。

```fish
curl -s localhost:8080/ping
curl -sN -X POST localhost:8080/invocations \
  -H 'content-type: application/json' \
  -d '{"threadId":"t1","runId":"r1","messages":[{"id":"1","role":"user","content":"直近5日のCPU使用率を見せて"}],"tools":[],"context":[],"state":{},"forwardedProps":{}}'
```

フロントを起動します。`AGENTCORE_RUNTIME_ARN` を設定しない場合は、`http://localhost:8080/invocations` に直接つなぎます（接続先は `AGUI_BACKEND_URL` で変更できます）。

```fish
cd frontend
npm ci
npm run dev
```

`http://localhost:3000` を開き、「直近7日のCPU使用率を見せて」のように聞くと、チャットの中に折れ線グラフが表示されます。

コンテナで確認する場合は次のとおりです。

```fish
cd agent
docker build --platform linux/arm64 -t rust-agent .
docker run --rm -p 8080:8080 -e AWS_REGION=us-west-2 \
  -e AWS_ACCESS_KEY_ID -e AWS_SECRET_ACCESS_KEY -e AWS_SESSION_TOKEN rust-agent
```

## デプロイ

```fish
cd cdk
npm ci
npm test
npx cdk deploy --all
```

- モデルは環境変数 `BEDROCK_MODEL_ID` で変更できます（デフォルトは `us.anthropic.claude-haiku-4-5-20251001-v1:0`）。
- デプロイ先のリージョンは `CDK_DEFAULT_REGION` で変更できます（デフォルトは us-west-2）。
- デプロイ後、`RustAgentFrontendStack.FunctionUrl` に出力された URL をブラウザで開きます。

**注意**: サンプルのため、Function URL は認証なし（`AuthType=NONE`）です。URL を知っていれば誰でも Bedrock を呼び出せるので、確認が終わったら `npx cdk destroy --all` で削除してください。

## 参考リポジトリから変えた点
- エージェントを Python（Flask と LangChain）から Rust に置き換えた。Athena、Memory、Claude Platform WIF はなくし、Bedrock ConverseStream を使う。
- CloudFront と署名付き Cookie（`proxy.ts`、`/api/access`、鍵の発行スクリプト）を削除し、Function URL で直接公開する。
- エージェント ID を `rust_agent` に変え、グラフを `MetricsChart`（`get_sample_metrics` の結果を描く折れ線グラフ）に差し替えた。
- テキストは最終回答をまとめて送るのではなく、トークン単位で送る。

## 未対応
- OpenTelemetry（ADOT の自動計装は Rust では使えないため、`opentelemetry-otlp` を自分で組み込む必要がある）
- AgentCore Gateway（rmcp）との連携と、AgentCore Memory
