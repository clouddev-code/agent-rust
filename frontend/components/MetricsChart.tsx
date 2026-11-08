"use client";

import {
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

// agent/src/tools.rs の SampleMetrics が返すJSONの形。
export type SampleMetrics = {
  metric: string;
  unit: string;
  points: { date: string; value: number }[];
};

const METRIC_LABELS: Record<string, string> = {
  cpu: "CPU使用率",
  latency: "p95レイテンシ",
  requests: "リクエスト数",
};

export function MetricsChart({ data }: { data: SampleMetrics }) {
  const label = METRIC_LABELS[data.metric] ?? data.metric;
  const format = (value: number) => `${value.toLocaleString("ja-JP")} ${data.unit}`;

  return (
    <div className="w-full rounded-lg border border-gray-200 p-4">
      <div className="mb-2 flex items-baseline justify-between">
        <h3 className="text-sm font-semibold text-gray-700">{label}（日別）</h3>
        <span className="text-xs text-gray-400">デモ用のダミーデータ</span>
      </div>
      <ResponsiveContainer width="100%" height={240}>
        <LineChart data={data.points} margin={{ left: 8, right: 24 }}>
          <CartesianGrid strokeDasharray="3 3" />
          <XAxis dataKey="date" tickFormatter={(date: string) => date.slice(5)} />
          <YAxis width={64} />
          <Tooltip formatter={(value) => [format(Number(value)), label]} />
          <Line type="monotone" dataKey="value" stroke="#2563eb" strokeWidth={2} dot={{ r: 3 }} />
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
}
