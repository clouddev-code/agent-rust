import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // Lambda Web Adapterコンテナ用に.next/standaloneへnode_modulesごと
  // 最小構成を出力する（frontend/Dockerfile参照）。
  output: "standalone",
};

export default nextConfig;
