import * as cdk from 'aws-cdk-lib/core';
import { Match, Template } from 'aws-cdk-lib/assertions';
import { BackendStack } from '../lib/backend-stack';

const MODEL_ID = 'us.anthropic.claude-haiku-4-5-20251001-v1:0';

function makeStack(): BackendStack {
  const app = new cdk.App();
  return new BackendStack(app, 'TestBackendStack', {
    env: { account: '123456789012', region: 'us-west-2' },
    modelId: MODEL_ID,
  });
}

test('creates a PUBLIC network / AGUI protocol AgentCore Runtime', () => {
  const template = Template.fromStack(makeStack());

  template.hasResourceProperties('AWS::BedrockAgentCore::Runtime', {
    AgentRuntimeName: 'rust_agent',
    ProtocolConfiguration: 'AGUI',
    NetworkConfiguration: { NetworkMode: 'PUBLIC' },
    EnvironmentVariables: Match.objectLike({ BEDROCK_MODEL_ID: MODEL_ID }),
  });
});

test('grants ConverseStream on the inference profile and its foundation model only', () => {
  const template = Template.fromStack(makeStack());

  template.hasResourceProperties('AWS::IAM::Policy', {
    PolicyDocument: {
      Statement: Match.arrayWith([
        Match.objectLike({
          Sid: 'BedrockConverseStream',
          Action: ['bedrock:InvokeModel', 'bedrock:InvokeModelWithResponseStream'],
          Resource: [
            `arn:aws:bedrock:us-west-2:123456789012:inference-profile/${MODEL_ID}`,
            'arn:aws:bedrock:*::foundation-model/anthropic.claude-haiku-4-5-20251001-v1:0',
          ],
        }),
      ]),
    },
  });
});
