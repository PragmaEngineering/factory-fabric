import assert from 'node:assert/strict';
import { pathToFileURL } from 'node:url';
import { randomUUID } from 'node:crypto';
const root = process.env.FABRIC_TEST_CONTROLS;
const { Client } = await import(pathToFileURL(`${root}/node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js`));
const { StdioClientTransport, getDefaultEnvironment } = await import(pathToFileURL(`${root}/node_modules/@modelcontextprotocol/sdk/dist/esm/client/stdio.js`));
const [binary, socket] = process.argv.slice(2);
async function connect(identity) {
  const client = new Client({ name: 'fabric-integration', version: '1' });
  const transport = new StdioClientTransport({ command: binary, args: ['mcp', socket, '--identity', identity], stderr: 'inherit', env: { ...getDefaultEnvironment(), ...Object.fromEntries(['FABRIC_TOKEN', 'FABRIC_CA_CERT'].filter(k => process.env[k]).map(k => [k, process.env[k]])) } });
  await client.connect(transport);
  return client;
}
async function call(client, name, args) {
  const result = await client.callTool({ name, arguments: args });
  assert.ok(!result.isError, JSON.stringify(result));
  return result.structuredContent.result;
}
const a = await connect(`operator-${randomUUID()}`);
const b = await connect(`external-${randomUUID()}`);
try {
  const tools = await a.listTools();
  assert.ok(tools.tools.some(t => t.name === 'agents_invoke'));
  assert.ok(tools.tools.some(t => t.name === 'comms_send_message'));
  const catalog = await call(a, 'agents_list', {});
  assert.ok(catalog.some(c => c.package === 'utility'));
  await call(a, 'agents_describe', { package: 'utility', capability: 'echo' });
  const accepted = await call(a, 'agents_invoke', { request_key: randomUUID(), package: 'utility', capability: 'echo', input: { _delay: 1 } });
  assert.equal(accepted.state, 'accepted');
  const denied = await b.callTool({ name: 'invocations_get', arguments: { id: accepted.id } });
  assert.equal(denied.isError, true);
  const completed = await call(a, 'invocations_wait', { id: accepted.id, seconds: 10 });
  assert.equal(completed.state, 'completed');
  assert.equal(completed.output._delay, 1);
  const invalid = await a.callTool({ name: 'agents_invoke', arguments: { request_key: randomUUID(), package: 'utility', capability: 'inspect', input: {} } });
  assert.equal(invalid.isError, true);
  const capability = { name: 'answer', description: 'Return a structured answer.', version: '1', input_schema: { type: 'object' }, output_schema: { type: 'object', required: ['answer'] } };
  const registration = await call(b, 'agents_register', { name: 'mcp-connected', capabilities: [capability], lease_seconds: 30 });
  assert.ok((await call(a, 'agents_list', {})).some(c => c.package === 'mcp-connected'));
  assert.equal((await a.listTools()).tools.length, tools.tools.length);
  const work = await call(a, 'agents_invoke', { request_key: randomUUID(), package: 'mcp-connected', capability: 'answer', input: { question: 'test' } });
  const inbox = await call(b, 'comms_wait_for_message', { thread_id: registration.mailbox, wait_seconds: 5 });
  const dispatch = inbox.messages.find(m => m.payload.kind === 'fabric.invoke').payload.parts.find(p => p.kind === 'data').data;
  const participants = await call(b, 'comms_list_participants', { thread_id: dispatch.thread_id });
  const operator = participants.participants.find(p => p.kind === 'peer' && p.name.startsWith('operator-'));
  assert.ok(operator);
  await call(b, 'comms_send_message', { operation_id: randomUUID(), thread_id: dispatch.thread_id, recipients: [operator.id], payload: { kind: 'question', version: 1, parts: [{ kind: 'text', text: 'Any additional context?' }] } });
  const conversation = await call(a, 'comms_read_messages', { thread_id: dispatch.thread_id });
  assert.ok(conversation.messages.some(m => m.payload.kind === 'question'));
  await call(a, 'comms_send_message', { operation_id: randomUUID(), thread_id: dispatch.thread_id, recipients: [registration.owner], payload: { kind: 'answer', version: 1, parts: [{ kind: 'text', text: 'Use the supplied input.' }] } });
  const received = await call(b, 'comms_read_messages', { thread_id: dispatch.thread_id });
  assert.ok(received.messages.some(m => m.payload.kind === 'answer'));
  await call(b, 'comms_send_message', { operation_id: randomUUID(), thread_id: dispatch.thread_id, recipients: [dispatch.result_recipient], payload: { kind: 'fabric.result', version: 1, parts: [{ kind: 'data', data: { invocation_id: work.id, output: { answer: 'complete' } } }] } });
  assert.equal((await call(a, 'invocations_wait', { id: work.id, seconds: 5 })).output.answer, 'complete');
  await call(b, 'agents_withdraw', { name: 'mcp-connected' });
  console.log('Official MCP client: discovery, async calls, isolation, dynamic registration and follow-up messages passed.');
} finally {
  await a.close();
  await b.close();
}
