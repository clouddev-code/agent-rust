import * as path from 'path';
import { CfnOutput, Stack, StackProps } from 'aws-cdk-lib';
import * as agentcore from 'aws-cdk-lib/aws-bedrockagentcore';
import * as assets from 'aws-cdk-lib/aws-ecr-assets';
import * as iam from 'aws-cdk-lib/aws-iam';
import { Construct } from 'constructs';

export interface BackendStackProps extends StackProps {
  /**
   * エージェントが呼ぶBedrockのクロスリージョン推論プロファイルID
   * （例: `us.anthropic.claude-haiku-4-5-20251001-v1:0`）。
   */
  readonly modelId: string;
}

/**
 * Rustエージェント（agent/）をBedrock AgentCore Runtime（networkMode=PUBLIC）
 * としてデプロイするスタック。
 */
export class BackendStack extends Stack {
  public readonly runtime: agentcore.Runtime;

  constructor(scope: Construct, id: string, props: BackendStackProps) {
    super(scope, id, props);

    const agentRuntimeArtifact = agentcore.AgentRuntimeArtifact.fromAsset(
      path.join(__dirname, '../../agent'),
      { platform: assets.Platform.LINUX_ARM64 },
    );

    this.runtime = new agentcore.Runtime(this, 'RustAgentRuntime', {
      runtimeName: 'rust_agent',
      agentRuntimeArtifact,
      // AG-UIプロトコル契約(/invocations, /ping, ポート8080)はHTTPと同一パス・ポートで、
      // AWS側が内部的に区別するのみ。Rust側はAG-UIイベントをSSEで返す。
      protocolConfiguration: agentcore.ProtocolType.AGUI,
      environmentVariables: {
        BEDROCK_MODEL_ID: props.modelId,
      },
    });

    // 推論プロファイルは、プロファイル自体と、ルーティング先の各リージョンの基盤モデルの
    // 両方への権限が必要。ルーティング先リージョンはプロファイルごとに異なるため、
    // 基盤モデルはリージョンをワイルドカードにしてモデルIDで絞る。
    const foundationModelId = props.modelId.replace(/^(us|eu|apac|jp|au|global)\./, '');
    this.runtime.addToRolePolicy(
      new iam.PolicyStatement({
        sid: 'BedrockConverseStream',
        actions: ['bedrock:InvokeModel', 'bedrock:InvokeModelWithResponseStream'],
        resources: [
          `arn:aws:bedrock:${this.region}:${this.account}:inference-profile/${props.modelId}`,
          `arn:aws:bedrock:*::foundation-model/${foundationModelId}`,
        ],
      }),
    );

    new CfnOutput(this, 'AgentRuntimeArn', { value: this.runtime.agentRuntimeArn });
  }
}
