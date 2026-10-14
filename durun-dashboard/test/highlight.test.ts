import assert from 'node:assert/strict';
import { test } from 'node:test';
import { highlightLine, languageOf } from '../client/highlight.ts';

test('Terraform のファイルは HCL として、ブロックの種類と属性の名前と文字列を色分けする', () => {
  assert.equal(languageOf('infra/main.tf'), 'hcl');
  assert.equal(languageOf('prod.tfvars'), 'hcl');
  assert.equal(
    highlightLine('resource "aws_s3_bucket" "b" {', 'hcl'),
    '<span class="hljs-keyword">resource</span> <span class="hljs-string">&quot;aws_s3_bucket&quot;</span> <span class="hljs-string">&quot;b&quot;</span> {',
  );
  assert.equal(highlightLine('  version = "~> 5.0"', 'hcl'), '  <span class="hljs-attr">version</span> = <span class="hljs-string">&quot;~&gt; 5.0&quot;</span>');
});
