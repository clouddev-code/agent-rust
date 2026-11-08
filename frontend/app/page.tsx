"use client";

import { CopilotChat, useRenderTool } from "@copilotkit/react-core/v2";
import { z } from "zod";

import { MetricsChart, type SampleMetrics } from "@/components/MetricsChart";

const sampleMetricsArgsSchema = z.object({
  metric: z.enum(["cpu", "latency", "requests"]),
  days: z.number().optional(),
});

export default function Home() {
  useRenderTool(
    {
      name: "get_sample_metrics",
      parameters: sampleMetricsArgsSchema,
      agentId: "rust_agent",
      render: ({ status, result }) => {
        if (status !== "complete") {
          return <p className="text-sm text-gray-500">メトリクスを取得しています…</p>;
        }
        const parsed: unknown = JSON.parse(result);
        if (typeof parsed !== "object" || parsed === null || !("points" in parsed)) {
          const message =
            typeof parsed === "object" && parsed !== null && "error" in parsed
              ? String((parsed as { error: unknown }).error)
              : "メトリクスの取得に失敗しました。";
          return (
            <div className="rounded-lg border border-red-200 bg-red-50 p-4 text-sm text-red-700">
              {message}
            </div>
          );
        }
        return <MetricsChart data={parsed as SampleMetrics} />;
      },
    },
    [],
  );

  return (
    <main className="mx-auto flex h-screen w-full max-w-3xl flex-col px-4 py-6">
      <h1 className="mb-1 text-xl font-semibold">Rust Agent on AgentCore</h1>
      <p className="mb-4 text-sm text-gray-600">
        Rustで実装したエージェントがAmazon Bedrock AgentCore Runtime上で応答します。
      </p>
      <div className="min-h-0 flex-1 overflow-hidden rounded-lg border border-gray-200">
        <CopilotChat className="h-full" />
      </div>
    </main>
  );
}
