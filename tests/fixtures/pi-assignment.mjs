#!/usr/bin/env node
// Model-free native Pi protocol fixture. Used only by integration tests.
import {readFileSync} from 'node:fs';
import {createInterface} from 'node:readline';
if(process.argv.includes('--version')) {console.log('1.0.3');process.exit(0);}
const profile=process.env.PI_CODING_AGENT_DIR;
const settings=JSON.parse(readFileSync(profile+'/settings.json','utf8'));
const providers=JSON.parse(readFileSync(profile+'/models.json','utf8')).providers;
const provider=settings.defaultProvider;
const model=providers[provider].models.find(m=>m.id===settings.defaultModel);
const selected={...model,provider,api:providers[provider].api,baseUrl:providers[provider].baseUrl};
let sequence=0;
async function rpc(method,params){
 const id=++sequence;
 const response=await fetch(process.env.AGENT_MCP_URL,{method:'POST',headers:{Authorization:'Bearer '+process.env.AGENT_MCP_TOKEN,'Content-Type':'application/json',Accept:'application/json, text/event-stream','MCP-Protocol-Version':'2025-03-26'},body:JSON.stringify({jsonrpc:'2.0',id,method,params})});
 const text=await response.text();let reply;try{reply=JSON.parse(text);}catch{reply=text.split('\n').filter(v=>v.startsWith('data:')).map(v=>JSON.parse(v.slice(5).trim())).find(v=>v.id===id);}
 if(!reply || reply.error || reply.result?.isError)throw new Error('assignment MCP rejected native fixture');
 return reply.result;
}
let initialized=false;
for await(const line of createInterface({input:process.stdin})) {
 const request=JSON.parse(line);
 if(request.type==='get_state') {console.log(JSON.stringify({type:'response',id:request.id,success:true,data:{model:selected,thinkingLevel:settings.defaultThinkingLevel}}));continue;}
 if(request.type==='get_available_models') {console.log(JSON.stringify({type:'response',id:request.id,success:true,data:{models:[selected]}}));continue;}
 if(Object.keys(process.env).some(key=>key.startsWith("FACTORY_")) || !process.env.AGENT_MCP_URL)process.exit(7);
 if(!initialized) {await rpc('initialize',{protocolVersion:'2025-03-26',capabilities:{},clientInfo:{name:'native-fixture',version:'1'}});initialized=true;}
 const result=await rpc('tools/call',{name:'assignment_get',arguments:{}});
 const assignment=result.structuredContent;
 const output={summary:'Native assignment completed',input:assignment.input,provider,model:model.id};
 if(assignment.input.delegate) {
  const {Client}=await import(new URL('../../../controls/node_modules/@modelcontextprotocol/sdk/dist/esm/client/index.js',import.meta.url));
  const {StdioClientTransport}=await import(new URL('../../../controls/node_modules/@modelcontextprotocol/sdk/dist/esm/client/stdio.js',import.meta.url));
  const configuration=JSON.parse(readFileSync(profile+'/mcp.json','utf8')).mcpServers.fabric;
  if(!configuration)throw new Error('delegation MCP was not provisioned');
  const fabric=new Client({name:'assigned-worker',version:'1'});
  await fabric.connect(new StdioClientTransport({command:configuration.command,args:configuration.args,env:{...process.env},stderr:'inherit'}));
  try {
   const call=async(name,args)=>{const v=await fabric.callTool({name,arguments:args});if(v.isError)throw new Error('worker delegation rejected: '+JSON.stringify(v));return v.structuredContent.result;};
   const child=await call('agents_invoke',{request_key:'native-child',package:'utility',capability:'echo',input:{finding:'delegated via assignment'}});
   const result=await call('invocations_wait',{id:child.id,seconds:15});
   if(result.state!=='completed')throw new Error('delegated work did not finish');
   output.child_id=child.id;output.child=result.output;
  } finally {await fabric.close();}
 }
 await rpc('tools/call',{name:'assignment_complete',arguments:{id:assignment.id,output}});
 console.log(JSON.stringify({type:'response',id:request.id,success:true}));
 console.log(JSON.stringify({type:'agent_settled'}));
}
