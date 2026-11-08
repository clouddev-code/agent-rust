#!/usr/bin/env node
import * as cdk from 'aws-cdk-lib/core';
import { BackendStack } from '../lib/backend-stack';
import { FrontendStack } from '../lib/frontend-stack';

const app = new cdk.App();

const env = {
  account: process.env.CDK_DEFAULT_ACCOUNT,
  region: process.env.CDK_DEFAULT_REGION ?? 'us-west-2',
};

// process.env参照はこのエントリポイントに閉じる（CDKベストプラクティス
// 「configuration via properties, env var lookups limited to the top level of the app」）。
const backendStack = new BackendStack(app, 'RustAgentBackendStack', {
  env,
  modelId: process.env.BEDROCK_MODEL_ID ?? 'us.anthropic.claude-haiku-4-5-20251001-v1:0',
});

new FrontendStack(app, 'RustAgentFrontendStack', {
  env,
  agentRuntime: backendStack.runtime,
});
