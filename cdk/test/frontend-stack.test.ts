import * as cdk from 'aws-cdk-lib/core';
import { Match, Template } from 'aws-cdk-lib/assertions';
import { BackendStack } from '../lib/backend-stack';
import { FrontendStack } from '../lib/frontend-stack';

function makeFrontendStack(): FrontendStack {
  const app = new cdk.App();
  const env = { account: '123456789012', region: 'us-west-2' };
  const backend = new BackendStack(app, 'TestBackendStack', {
    env,
    modelId: 'us.anthropic.claude-haiku-4-5-20251001-v1:0',
  });
  return new FrontendStack(app, 'TestFrontendStack', {
    env,
    agentRuntime: backend.runtime,
  });
}

test('Function URL uses RESPONSE_STREAM invoke mode and no CloudFront is created', () => {
  const template = Template.fromStack(makeFrontendStack());

  template.hasResourceProperties('AWS::Lambda::Url', {
    AuthType: 'NONE',
    InvokeMode: 'RESPONSE_STREAM',
  });
  template.resourceCountIs('AWS::CloudFront::Distribution', 0);
});

test('Lambda is ARM64, knows the runtime ARN and may invoke the runtime', () => {
  const template = Template.fromStack(makeFrontendStack());

  template.hasResourceProperties('AWS::Lambda::Function', {
    Architectures: ['arm64'],
    Environment: {
      Variables: { AGENTCORE_RUNTIME_ARN: Match.anyValue() },
    },
  });
  template.hasResourceProperties('AWS::IAM::Policy', {
    PolicyDocument: {
      Statement: Match.arrayWith([
        Match.objectLike({ Action: 'bedrock-agentcore:InvokeAgentRuntime' }),
      ]),
    },
  });
});
