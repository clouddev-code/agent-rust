# Rust 版 AgentCore Runtime サンプルの構成案（2026-11-08）

前提資料: `AgentCore-Runtime-Rust実装可否-20261108.md`
フロントの参考: https://github.com/clouddev-code/aws-cost-visual-report（以下「参考リポジトリ」）

## 結論
- **エージェント**: フレームワークは使わない。axum と `aws-sdk-bedrockruntime` の ConverseStream で、ツール呼び出しのループを自分で書く。
- **プロトコル**: 参考リポジトリと同じ **AG-UI** にする（Runtime は `ProtocolType.AGUI`）。Rust 側が AG-UI のイベントを SSE で返せば、参考リポジトリのフロント（Next.js + CopilotKit）と CDK の Frontend スタックはほぼそのまま使える。
- **ビルド**: 開発機が Apple Silicon（arm64）なので、`docker buildx --platform linux/arm64` をそのまま使う。クロスビルド用のツールは要らない。

## 1. エージェントフレームワークの比較

| 候補 | 最新版 / 日付 | Converse | ストリーミング | ツールループ | MCP | 評価 |
|---|---|---|---|---|---|---|
| **自前ループ（aws-sdk-bedrockruntime）** | 1.153.0 / 2026-10-30 | ○ | ○ ConverseStream | 自分で書く（100〜200 行） | rmcp を自分でつなぐ | **採用**。API が安定していて依存が少ない |
| rig（rig-core / rig-agent / rig-bedrock / rig-rmcp） | 0.46.0 / 2026-10-31 | ○ | ○ | ○ | ○ | 機能は最も充実。ただし 0.4x 系はほぼ毎回破壊的変更があり、MSRV は 1.97.1 |
| everruns-serve-agentcore | 0.51.0 / 2026-11-06 | feature あり | ○ AG-UI SSE | ○ | 未確認 | Experimental。依存にはせず、実装を読む参考にとどめる |
| adk-rust | 2.3.0 / 2026-10-09 | ○（品質は未確認） | 未確認 | ○ | 未確認 | 見送り |
| langchain-rust | 8.0.0 / 2026-11-03 | ○ | 未確認 | ○ | 未確認 | 2 週間でメジャー版が 5→8 に上がっており不安定 |
| genai | 0.7.3 / 2026-11-03 | ○ | ○ | × | × | LLM クライアントのみで、エージェントループはない |
| autoagents / swarms-rs / llm-chain | — | × / 未確認 / 開発停止 | — | — | — | 対象外 |

**自前ループにする理由**
1. rig は API がすぐ変わる。dev.to の実例記事（2025-11）のコードも、すでに今の API では動かない。サンプルとして長く使えない。
2. AgentCore 固有の部分（AG-UI のイベント形状、`/ping`、セッション ID ヘッダ）を SDK 呼び出しのすぐ近くに書けるので、説明しやすい。
3. Converse のツール呼び出しの仕組みがそのまま読めるので、教材として使える。

フレームワークを使う版も見せたい場合は、あとから rig を `=0.46.0` に固定した別ディレクトリとして追加する。

## 2. 採用スタック

| 層 | 採用 | バージョン |
|---|---|---|
| Web | axum（`response::sse::{Sse, Event, KeepAlive}`） | 0.8.9 |
| 非同期ランタイム | tokio | 最新の 1.x |
| LLM | aws-sdk-bedrockruntime（ConverseStream） | 1.153.0 |
| AWS 設定 | aws-config | 1.13.0 |
| シリアライズ | serde / serde_json（AG-UI は camelCase） | — |
| ログ | tracing / tracing-subscriber（JSON 出力） | — |
| MCP（第 2 段階） | rmcp（`transport-streamable-http-client-reqwest`） | 3.9.0 |
| TLS | rustls（ランタイムイメージに libssl を入れなくて済む） | — |
| ベースイメージ | ビルド: `rust:1.9x-bookworm` / 実行: `gcr.io/distroless/cc-debian12`（arm64） | — |
| フロント | 参考リポジトリを流用（Next.js 16.3 / React 19.2 / CopilotKit 1.67.1 / @ag-ui/client 0.0.57 / Recharts 3.10 / Tailwind v4） | — |
| IaC | CDK（TypeScript、`aws-cdk-lib/aws-bedrockagentcore` の stable 版 `Runtime`） | aws-cdk-lib ^2.266.0 |

## 3. 全体構成（参考リポジトリから CloudFront を除いたもの）

```
Browser ─→ Lambda Function URL (RESPONSE_STREAM, AuthType=NONE)   ※サンプルのため CloudFront と署名付き Cookie は省略（2026-11-08 決定）
  └ Next.js standalone on Lambda Web Adapter: /api/copilotkit（CopilotRuntime）
       └ AgentCoreHttpAgent → InvokeAgentRuntime（SigV4）
            └ AgentCore Runtime（AGUI）→ Rust コンテナ POST /invocations（SSE）
                 └ Bedrock ConverseStream（ツールループ）
```

## 4. Rust エージェントが守る契約

### エンドポイント
| メソッド / パス | 内容 |
|---|---|
| `GET /ping` | `{"status":"Healthy"}` を返す。ターン処理中は `HealthyBusy`。先頭は大文字。dev.to 記事の `"healthy"` は間違い |
| `POST /invocations` | AG-UI の `RunAgentInput` を受け取り、`text/event-stream` で応答する |
| 待ち受け | `0.0.0.0:8080` |

### リクエスト（`RunAgentInput`、camelCase）
```json
{ "threadId": "...", "runId": "...",
  "messages": [{ "id": "...", "role": "user|assistant|tool|system|developer", "content": "...",
                 "toolCalls": [{ "id": "...", "type": "function", "function": { "name": "...", "arguments": "{...}" } }],
                 "toolCallId": "..." }],
  "tools": [], "context": [], "state": {}, "forwardedProps": {} }
```
- 会話履歴は毎回 `messages` に全部入ってくる。Rust 側ではこれを Converse の `Message` 列に変換する。サーバー側に履歴を持たない。
- `X-Amzn-Bedrock-AgentCore-Runtime-Session-Id` ヘッダはログに出す。値はフロントが `threadId` を 33 文字まで右を `0` で埋めたもの。

### SSE イベント（送る順番）
フィールド名は推測で書かず、参考リポジトリの `backend/app/agui/events.py` と `views.py`、および `@ag-ui/client` 0.0.57 の型定義をそのまま写す。
```
data: {"type":"RUN_STARTED","threadId":"...","runId":"..."}
data: {"type":"TOOL_CALL_START","toolCallId":"...","toolCallName":"..."}
data: {"type":"TOOL_CALL_ARGS","toolCallId":"...","delta":"{...}"}
data: {"type":"TOOL_CALL_END","toolCallId":"..."}
data: {"type":"TOOL_CALL_RESULT","messageId":"...","toolCallId":"...","content":"<JSON文字列>","role":"tool"}
data: {"type":"TEXT_MESSAGE_START","messageId":"...","role":"assistant"}
data: {"type":"TEXT_MESSAGE_CONTENT","messageId":"...","delta":"..."}   ← ConverseStream のテキスト差分ごとに送る
data: {"type":"TEXT_MESSAGE_END","messageId":"..."}
data: {"type":"RUN_FINISHED","threadId":"...","runId":"..."}
: ping                                                                  ← 10 秒ごと（axum の KeepAlive）
```
- 参考リポジトリは最終回答を 1 回の CONTENT でまとめて送っている。Rust 版は ConverseStream の `ContentBlockDelta(Text)` をトークン単位で `TEXT_MESSAGE_CONTENT` として送る。
- ストリームを始めたあとにエラーが起きた場合は、HTTP 200 のまま `{"type":"RUN_ERROR","message":"..."}` を送って終わる。

### ツールループ
1. `converse_stream()` に、system・messages・`tool_config` を渡して呼ぶ。
2. ストリームのイベントを順に処理する。テキストの差分はそのまま SSE で送る。ツールの入力は文字列として連結していく。
3. `StopReason::ToolUse` なら、ツールを実行して `TOOL_CALL_*` と `TOOL_CALL_RESULT` を送る。続けて `ToolResult` を履歴に追加し、1 に戻る。`EndTurn` なら終了する。
4. 無限ループにならないよう、最大反復回数（例: 8）を設ける。
5. `aws_smithy_types::Document` と `serde_json::Value` の相互変換は、専用のモジュールにまとめる。

## 5. デモ用ツール（グラフ表示の確認用）
参考リポジトリのフロントは、`useRenderTool` でツールの結果を Recharts のグラフにする。この部分を実際に動かすため、外部依存のないツールを 1〜2 個用意する。

| ツール名 | 引数 | 返り値（JSON 文字列） | フロントの描画 |
|---|---|---|---|
| `get_sample_metrics` | `{ "metric": "cpu|latency", "days": 7 }` | `[{"date":"2026-11-01","value":42.1}, ...]`（決まった規則で生成したダミー値） | 折れ線グラフ |
| `get_current_time` | なし | `{"iso":"..."}` | テキストのみ |

- 参考リポジトリの Cost Explorer や Athena の機能は移植しない（AWS の権限と CUR が必要になるため）。
- 本物のデータを使いたくなったら、`ce:GetCostAndUsage` を呼ぶツールを `aws-sdk-costexplorer` で追加する（任意）。

## 6. ディレクトリ構成案

```
AgentCore-Runtime-Rust/
├── agent/                    # Rust エージェント
│   ├── Cargo.toml
│   ├── Dockerfile            # マルチステージ。rust:bookworm → distroless/cc-debian12、linux/arm64
│   └── src/
│       ├── main.rs           # axum ルーター（/ping, /invocations）
│       ├── agui.rs           # RunAgentInput と AG-UI イベントの型（serde, camelCase）
│       ├── agent_loop.rs     # ConverseStream のツールループ → イベントの Stream
│       ├── convert.rs        # AG-UI Message ⇔ Converse Message、Document ⇔ serde_json
│       └── tools.rs          # Tool trait とデモ用ツール
├── frontend/                 # 参考リポジトリの frontend/ を流用
├── cdk/                      # BackendStack（Rust Runtime）と FrontendStack
└── docs/
```

## 7. 参考リポジトリから変える箇所

| 場所 | 変更内容 |
|---|---|
| `cdk/lib/backend-stack.ts` | `AgentRuntimeArtifact.fromAsset` の参照先を `../../agent` にする。Athena、Glue、Memory、WIF の props・IAM・環境変数を削除する。`bedrock:InvokeModelWithResponseStream` と `bedrock:InvokeModel`（推論プロファイルと基盤モデルの ARN）を追加する。`protocolConfiguration: AGUI` は変えない |
| `cdk/bin/cdk.ts` | CUR と WIF の必須環境変数を削除し、`BEDROCK_MODEL_ID` を追加する |
| `frontend/app/page.tsx` | `useRenderTool` の `name` とスキーマを `get_sample_metrics` に合わせる。グラフのコンポーネントを差し替える |
| `frontend/app/layout.tsx` / `app/api/copilotkit/route.ts` | エージェント ID `cost_analyst` を `rust_agent` などに変える |
| ローカル開発 | `route.ts` の `HttpAgent` の URL を `http://localhost:8080/invocations` にする（AGENTCORE_RUNTIME_ARN を設定しない場合） |
| CloudFront 関連 | `proxy.ts`、`app/api/access`、`cdk/scripts/issue-access-url.mjs`、鍵ファイルを削除し、FrontendStack は Function URL だけにする |
| 変えない部分 | `lib/agentcore-agent.ts`（InvokeAgentRuntime、33 文字のパディング、409 での再試行）、Lambda Web Adapter |

## 8. 作業手順（承認後に進める）
1. **Rust エージェント**: `/ping` と `/invocations` を実装し、ツールなしのテキストストリーミングを動かす。`curl -N` で SSE を確認する。
2. **ツールループ**: デモ用ツールを足し、`TOOL_CALL_*` の順番を確認する。
3. **コンテナ**: `docker buildx build --platform linux/arm64` でビルドする。ローカルで `docker run -p 8080:8080` して `curl` で確認する。認証情報は環境変数で渡す。
4. **フロント**: 参考リポジトリの frontend をコピーして書き換え、`localhost:8080` に直接つないで E2E を確認する。
5. **CDK**: Backend と Frontend の 2 スタックを書き換えて `cdk synth` を通す。`cdk deploy` のコマンドを用意し、デプロイはユーザーが実行する。
6. （第 2 段階）OTEL（`opentelemetry-otlp`）、rmcp で Gateway 連携、rig 版の追加。

## 9. 未確認・決めること
- **Bedrock のモデル ID**: 推論プロファイル（`us.` / `jp.` / `global.` のどれか）とモデルを決める。環境変数で受け取る形にする。
- **リージョン**: 参考リポジトリは us-west-2 がデフォルト。サンプルで ap-northeast-1 を使うかどうか。
- **Rust ツールチェーンのバージョン**: aws-sdk-bedrockruntime の MSRV は 1.94.1。ビルドイメージのタグはビルド時に確認する。
- **rmcp から SigV4 認証の Gateway を呼べるか**: 未確認。JWT 認証なら `auth_header` で対応できる。
- **`HealthyBusy`**: ストリームを処理している間に返すと Runtime のスケーリングにどう影響するか。カウンタで管理する予定。
- **CopilotKit が `TEXT_MESSAGE_CONTENT` のトークン単位の差分をそのまま表示できるか**: AG-UI の仕様上は問題ないはずだが、実際に動かして確認する。

## 参考
- 参考リポジトリのうち、読む対象のファイル: `frontend/lib/agentcore-agent.ts`、`frontend/app/page.tsx`、`backend/app/agui/events.py`、`backend/app/agui/views.py`、`cdk/lib/backend-stack.ts`、`cdk/lib/frontend-stack.ts`、`research/ai-agentcore-runtime-agui-protocol-contract.md`
- https://crates.io/crates/aws-sdk-bedrockruntime
- https://docs.rs/axum/0.8.9/axum/response/sse/
- https://crates.io/crates/rmcp
- https://github.com/0xPlaygrounds/rig
- https://crates.io/crates/everruns-serve-agentcore
- https://dev.to/aws-builders/deploy-rust-agent-to-aws-agentcore-runtime-with-github-actions-3gf8
- https://docs.aws.amazon.com/bedrock-agentcore/latest/devguide/runtime-http-protocol-contract.html
