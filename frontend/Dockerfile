# Lambda Web Adapterでラップし、Next.js standaloneサーバーをLambdaコンテナ
# イメージとして動かす。ARM64ビルドは`cdk/`側でDockerImageAsset
# (platform: Platform.LINUX_ARM64)により指定する。

FROM node:22-slim AS deps
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci

FROM node:22-slim AS build
WORKDIR /app
COPY --from=deps /app/node_modules ./node_modules
COPY . .
RUN npm run build

FROM node:22-slim AS runtime
COPY --from=public.ecr.aws/awsguru/aws-lambda-adapter:1.0.1 /lambda-adapter /opt/extensions/lambda-adapter
WORKDIR /app
COPY --from=build /app/.next/standalone ./
COPY --from=build /app/.next/static ./.next/static
COPY --from=build /app/public ./public

ENV PORT=8080
ENV HOSTNAME=0.0.0.0
ENV AWS_LWA_INVOKE_MODE=response_stream
ENV AWS_LWA_PORT=8080

EXPOSE 8080

CMD ["node", "server.js"]
