import { AbstractAgent, transformHttpEventStream } from "@ag-ui/client";
import type { AgentConfig, BaseEvent, RunAgentInput } from "@ag-ui/client";
import {
  BedrockAgentCoreClient,
  InvokeAgentRuntimeCommand,
  RetryableConflictException,
} from "@aws-sdk/client-bedrock-agentcore";
import { Observable } from "rxjs";
import type { Readable } from "node:stream";

const MIN_RUNTIME_SESSION_ID_LENGTH = 33;

interface AgentCoreHttpAgentConfig extends AgentConfig {
  agentRuntimeArn: string;
}

/**
 * CopilotKitの`HttpAgent`（HTTP直叩き前提）の代わりに、SigV4署名済みの
 * `InvokeAgentRuntime`ストリーミング呼び出しでBedrock AgentCore Runtime（backend）
 * に接続するアダプタ。`@ag-ui/client`が`HttpEvent`/`HttpEventType`を外部公開して
 * いないため、`transformHttpEventStream`への入力はダックタイピングでの`as never`
 * キャストを介して渡す（ランタイム上の必須プロパティはminifiedソースを直接確認済み:
 * `{type: "headers", status, headers}` / `{type: "data", data}`）。
 */
export class AgentCoreHttpAgent extends AbstractAgent {
  private agentRuntimeArn: string;
  private client: BedrockAgentCoreClient;

  constructor(config: AgentCoreHttpAgentConfig) {
    super(config);
    this.agentRuntimeArn = config.agentRuntimeArn;
    this.client = new BedrockAgentCoreClient({});
  }

  /**
   * CopilotKitランタイムはリクエストごとに`agents[agentId].clone()`でエージェントを
   * 複製してから`run()`を呼ぶ（`@copilotkit/runtime`の`cloneAgentForRequest`）。
   * `AbstractAgent.clone()`は`Object.create(prototype)`でベースクラスのフィールドしか
   * コピーしないため、オーバーライドしてサブクラス固有フィールドを引き継ぐ
   * （`@ag-ui/client`の`HttpAgent.clone()`と同じパターン）。これを怠ると複製後の
   * インスタンスで`this.client`が`undefined`になり、実行時に失敗する。
   */
  clone(): AgentCoreHttpAgent {
    const cloned = super.clone() as AgentCoreHttpAgent;
    cloned.agentRuntimeArn = this.agentRuntimeArn;
    cloned.client = this.client;
    return cloned;
  }

  run(input: RunAgentInput): Observable<BaseEvent> {
    const runtimeSessionId = toRuntimeSessionId(input.threadId);
    const payload = new TextEncoder().encode(JSON.stringify(input));

    const httpEvents$ = new Observable((subscriber) => {
      let cancelled = false;

      (async () => {
        try {
          const response = await invokeWithSessionBusyRetry(this.client, {
            agentRuntimeArn: this.agentRuntimeArn,
            runtimeSessionId,
            contentType: "application/json",
            accept: "text/event-stream",
            payload,
          });

          subscriber.next({
            type: "headers",
            status: response.statusCode ?? 200,
            headers: new Headers({
              "content-type": response.contentType ?? "text/event-stream",
            }),
          });

          // Lambda(Node.js)ランタイムではSdkStream<IncomingMessage | Readable>。
          // ブラウザ向けの型（ReadableStream/Blob）はSymbol.asyncIteratorを
          // 静的に持たないため、Node.js側の型に絞ってキャストする。
          const body = response.response as unknown as Readable | undefined;
          if (!body) {
            throw new Error("AgentCore Runtimeからのレスポンスにストリームボディがありません");
          }
          for await (const chunk of body) {
            if (cancelled) break;
            subscriber.next({
              type: "data",
              data: chunk instanceof Uint8Array ? chunk : new Uint8Array(chunk),
            });
          }
          subscriber.complete();
        } catch (error) {
          subscriber.error(error);
        }
      })();

      return () => {
        cancelled = true;
      };
    });

    return transformHttpEventStream(httpEvents$ as never);
  }
}

async function invokeWithSessionBusyRetry(
  client: BedrockAgentCoreClient,
  input: ConstructorParameters<typeof InvokeAgentRuntimeCommand>[0],
) {
  try {
    return await client.send(new InvokeAgentRuntimeCommand(input));
  } catch (error) {
    if (error instanceof RetryableConflictException) {
      return await client.send(new InvokeAgentRuntimeCommand(input));
    }
    throw error;
  }
}

/**
 * AgentCore Runtimeの`runtimeSessionId`は33文字以上が要求される
 * (X-Amzn-Bedrock-AgentCore-Runtime-Session-Id)。CopilotKitの`threadId`は
 * 通常UUID(36文字)だが、短い値が来ても同一threadIdなら常に同じセッションIDに
 * なるよう決定的にパディングする。
 */
function toRuntimeSessionId(threadId: string): string {
  if (threadId.length >= MIN_RUNTIME_SESSION_ID_LENGTH) {
    return threadId;
  }
  return threadId.padEnd(MIN_RUNTIME_SESSION_ID_LENGTH, "0");
}
