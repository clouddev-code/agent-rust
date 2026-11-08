import { CopilotRuntime, InMemoryAgentRunner, createCopilotHonoHandler } from "@copilotkit/runtime/v2";
import { HttpAgent } from "@ag-ui/client";
import type { AbstractAgent } from "@ag-ui/client";
import { handle } from "hono/vercel";
import { AgentCoreHttpAgent } from "@/lib/agentcore-agent";

// 本番（AgentCore Runtime, publicエンドポイント）ではSigV4署名済みの
// InvokeAgentRuntimeで接続する。ローカル開発ではRustエージェント（agent/）の
// /invocations に直接HTTP接続する（README.md「ローカルで動かす」参照）。
const AGENTCORE_RUNTIME_ARN = process.env.AGENTCORE_RUNTIME_ARN;
const AGUI_BACKEND_URL = process.env.AGUI_BACKEND_URL ?? "http://localhost:8080/invocations";

const rustAgent: AbstractAgent = AGENTCORE_RUNTIME_ARN
  ? new AgentCoreHttpAgent({ agentRuntimeArn: AGENTCORE_RUNTIME_ARN })
  : new HttpAgent({ url: AGUI_BACKEND_URL });

const runtime = new CopilotRuntime({
  agents: {
    rust_agent: rustAgent,
  },
  runner: new InMemoryAgentRunner(),
});

const app = createCopilotHonoHandler({
  runtime,
  basePath: "/api/copilotkit",
  mode: "single-route",
});

export const POST = handle(app);
