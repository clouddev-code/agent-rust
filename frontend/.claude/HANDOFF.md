# HANDOFF（2026-11-08）

対象はリポジトリ全体（`/Users/hiruta/work/AgentCore-Runtime-Rust`）。frontend だけの話ではない。

## 完了したこと
- 構成を検討した資料: `../Rust版AgentCoreサンプル-構成検討-20261108.md`。手順は `../README.md` にある。
- `agent/`（Rust）
  - axum 0.8.9、aws-sdk-bedrockruntime 1.153（ConverseStream）で、ツール呼び出しのループを自分で書いた。
  - 応答は AG-UI の SSE で返す。`/ping` は処理中なら HealthyBusy、そうでなければ Healthy を返す。
  - ツールは `get_sample_metrics` と `get_current_time`。
  - テスト 10 件と clippy が通った。ローカルと Docker（arm64、51.6MB）の両方で Bedrock を呼べることを確認した。
- `frontend/`
  - 参考リポジトリ（clouddev-code/aws-cost-visual-report）の frontend を流用した。
  - エージェント ID を `rust_agent` にし、グラフを `components/MetricsChart.tsx` に差し替えた。
  - ローカルでの接続先は `http://localhost:8080/invocations`。lint とビルドが通った。CopilotKit を経由して curl で E2E を確認した。
- `cdk/`
  - BackendStack: Runtime を AGUI で作り、IAM で推論プロファイルと基盤モデルの呼び出しを許可した。
  - FrontendStack: Lambda の Function URL のみ（AuthType=NONE、RESPONSE_STREAM）。
  - テスト 4 件と synth が通った。

## 重要な決定事項
- リージョンは us-west-2。モデルは `us.anthropic.claude-haiku-4-5-20251001-v1:0`。
- エージェントのフレームワーク（rig など）は使わない。rig は破壊的変更が多いため。
- CloudFront と署名付き Cookie は使わない（ユーザーの指示）。`proxy.ts`、`/api/access`、`scripts/`、`keys/` は削除済み。
- aws-config の `credentials-login` feature を有効にした（ローカルで `aws login` を使うため）。
- `frontend/AGENTS.md` と `frontend/CLAUDE.md` は `next dev` が自動で生成するもの。残してある。

## TODO
- [ ] ブラウザで `localhost:3000` を開き、グラフが表示されるか確認する（未検証）。
- [ ] ユーザーが `cd cdk; npx cdk deploy --all` を実行し、デプロイ後の E2E を確認する。デプロイを Claude が実行してはいけない。
- [ ] 確認が終わったら `npx cdk destroy --all` を実行する（Function URL は認証なしのため）。
- [ ] 1 回の応答でテキストのあとにツールを呼ぶケースを確認する（未検証）。
- [ ] 第 2 段階: OTEL、rmcp を使った Gateway 連携、rig 版。
