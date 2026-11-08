import * as path from 'path';
import { CfnOutput, Duration, Stack, StackProps } from 'aws-cdk-lib';
import type * as agentcore from 'aws-cdk-lib/aws-bedrockagentcore';
import * as assets from 'aws-cdk-lib/aws-ecr-assets';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import { Construct } from 'constructs';

export interface FrontendStackProps extends StackProps {
  /** backend(AgentCore Runtime)。frontend LambdaにInvokeAgentRuntime権限を付与する。 */
  readonly agentRuntime: agentcore.Runtime;
}

/**
 * frontend(Next.js)をLambda Web Adapterコンテナ + Function URL(RESPONSE_STREAM)で
 * 公開するスタック。構成は clouddev-code/aws-cost-visual-report を流用し、サンプル用に
 * CloudFrontと署名付きCookieを省いている。
 *
 * Function URLはAuthType=NONEのため、URLを知っていれば誰でもエージェント（Bedrock）を
 * 呼び出せる。検証が終わったらスタックを削除すること。
 */
export class FrontendStack extends Stack {
  constructor(scope: Construct, id: string, props: FrontendStackProps) {
    super(scope, id, props);

    const fn = new lambda.DockerImageFunction(this, 'FrontendFunction', {
      functionName: 'rust-agent-frontend',
      code: lambda.DockerImageCode.fromImageAsset(path.join(__dirname, '../../frontend'), {
        platform: assets.Platform.LINUX_ARM64,
      }),
      architecture: lambda.Architecture.ARM_64,
      memorySize: 1024,
      timeout: Duration.minutes(2),
      environment: {
        AGENTCORE_RUNTIME_ARN: props.agentRuntime.agentRuntimeArn,
      },
    });

    // frontend LambdaがSigV4署名済みのInvokeAgentRuntimeでbackendを呼び出すための権限
    // （X-Amzn-Bedrock-AgentCore-Runtime-User-Idは使わないためInvokeRuntimeのみ付与）。
    props.agentRuntime.grantInvokeRuntime(fn);

    // SSEをバッファせずにブラウザへ流すためRESPONSE_STREAMにする
    // （frontend/DockerfileのAWS_LWA_INVOKE_MODE=response_streamと対）。
    const functionUrl = fn.addFunctionUrl({
      authType: lambda.FunctionUrlAuthType.NONE,
      invokeMode: lambda.InvokeMode.RESPONSE_STREAM,
    });

    new CfnOutput(this, 'FunctionUrl', { value: functionUrl.url });
  }
}
