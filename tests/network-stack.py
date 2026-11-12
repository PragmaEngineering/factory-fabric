"""Separate-process HTTPS integration: python3 tests/network-stack.py after build."""
import contextlib
import http.client
import http.server
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import ssl
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request

REPO = Path(__file__).resolve().parents[1]
BINARY = REPO / "bin/fabric"

def free_port():
    with socket.socket() as connection:
        connection.bind(("127.0.0.1", 0))
        return connection.getsockname()[1]

with tempfile.TemporaryDirectory(prefix="fab-net-", dir="/tmp") as directory, contextlib.ExitStack() as cleanup:
    root = Path(directory)
    host = root / "execution-host"
    operator = root / "operator"
    host.mkdir(); operator.mkdir()
    cert = root / "certificate.pem"
    key = root / "key.pem"
    settings = root / "openssl.cnf"
    settings.write_text("[req]\ndistinguished_name=dn\nx509_extensions=ext\nprompt=no\n[dn]\nCN=localhost\n[ext]\nsubjectAltName=DNS:localhost\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign,digitalSignature,keyEncipherment\n")
    subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", str(key), "-out", str(cert), "-days", "1", "-config", str(settings)], check=True, capture_output=True)
    server_key, server_cert = root / "server-key.pem", root / "server-certificate.pem"
    csr, extensions = root / "server.csr", root / "server.ext"
    extensions.write_text("subjectAltName=DNS:localhost\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n")
    subprocess.run(["openssl","req","-new","-newkey","rsa:2048","-nodes","-keyout",str(server_key),"-out",str(csr),"-subj","/CN=localhost"],check=True,capture_output=True)
    subprocess.run(["openssl","x509","-req","-in",str(csr),"-CA",str(cert),"-CAkey",str(key),"-CAcreateserial","-out",str(server_cert),"-days","1","-extfile",str(extensions)],check=True,capture_output=True)
    tls = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    tls.load_cert_chain(server_cert, server_key)
    client_tls = ssl.create_default_context(cafile=str(cert))

    def proxy(origin):
        class Forward(http.server.BaseHTTPRequestHandler):
            def do_POST(self): self.forward()
            def do_GET(self): self.forward()
            def log_message(self, *_): pass
            def forward(self):
                connection = http.client.HTTPConnection("127.0.0.1", origin, timeout=45)
                try:
                    body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
                    headers = {k:v for k,v in self.headers.items() if k.lower() not in ("connection", "transfer-encoding")}
                    connection.request(self.command, self.path, body, headers)
                    response = connection.getresponse()
                    data = response.read()
                    self.send_response(response.status)
                    self.send_header("Content-Type", response.getheader("Content-Type", "application/json"))
                    self.send_header("Content-Length", str(len(data)))
                    self.end_headers(); self.wfile.write(data)
                except (OSError, http.client.HTTPException):
                    self.send_error(502)
                finally:
                    connection.close()
        server = http.server.ThreadingHTTPServer(("127.0.0.1",0),Forward)
        server.socket = tls.wrap_socket(server.socket,server_side=True)
        threading.Thread(target=server.serve_forever,daemon=True).start()
        cleanup.callback(server.server_close); cleanup.callback(server.shutdown)
        return f"https://localhost:{server.server_port}"

    controls_port, fabric_port = free_port(), free_port()
    controls_url, fabric_url = proxy(controls_port), proxy(fabric_port)
    host_token, fabric_token = "host-"+os.urandom(32).hex(), "operator-"+os.urandom(32).hex()
    (host / "token").write_text(host_token)
    (operator / "token").write_text(fabric_token)
    env = {**os.environ, "CONTROLS_TOKEN":host_token, "FABRIC_TOKEN":fabric_token, "FABRIC_CA_CERT":str(cert), "SSL_CERT_FILE":str(cert), "NODE_EXTRA_CA_CERTS":str(cert), "PATH":str(REPO/"bin")+os.pathsep+os.environ["PATH"]}
    host_env = {**env, "FABRIC_TEST_SECRET":"execution-host"}
    # Model independent packages are installed only as execution-host packages.
    shutil.copytree(REPO/"examples",host/"packages")
    config = {"version":1,"name":"network-smoke", "controls_endpoint":controls_url,"fabric_endpoint":fabric_url,"concurrency":4,"packages":[]}
    for name,entry,capability,delegation,args in [("utility","echo.sh","echo",False,[]),("reviewer","reviewer.sh","review",True,["fabric"])]:
        config["packages"].append({"name":name,"path":str(host/"packages"),"entrypoint":entry,"args":args,"working_directory":str(host),"allow_delegation":delegation,"environment":{"HOST_VALUE":"FABRIC_TEST_SECRET"},"capabilities":[{"name":capability,"description":"network fixture","version":"1","input_schema":{"type":"object"},"output_schema":{"type":"object"}}]})
    # Include the schema used by the shared official MCP test.
    config["packages"][0]["capabilities"].append({"name":"inspect","description":"inspect","version":"1","input_schema":{"type":"object","required":["task"]},"output_schema":{"type":"object"}})
    class Provider(http.server.BaseHTTPRequestHandler):
        def log_message(self,*_): pass
        def do_POST(self):
            body=(REPO.parent/"registry/tests/fixtures/readiness-response.json").read_bytes()
            self.send_response(200);self.send_header("Content-Type","application/json");self.send_header("Content-Length",str(len(body)));self.end_headers();self.wfile.write(body)
    provider=http.server.ThreadingHTTPServer(("127.0.0.1",0),Provider)
    threading.Thread(target=provider.serve_forever,daemon=True).start()
    cleanup.callback(provider.server_close);cleanup.callback(provider.shutdown)
    config["packages"].append({"name":"readiness","protocol":"mcp","path":str(REPO.parent/"registry/classifiers/task-readiness"),"entrypoint":"start.sh","working_directory":str(host),"environment":{"OPENROUTER_API_KEY":"FABRIC_TEST_SECRET"},"overrides":{"endpoint":f"http://127.0.0.1:{provider.server_port}"},"capabilities":[{"name":"assess","description":"Readiness fixture","version":"1","input_schema":{"type":"object"},"output_schema":{"type":"object"}}]})
    config["packages"].append({"name":"architect","protocol":"mcp","runtime":{"name":"pi","version":"=1.0.3","executable":str(REPO/"tests/fixtures/pi-assignment.mjs")},"allow_delegation":True,"path":str(REPO.parent/"registry/agents/architect"),"entrypoint":"start.sh","working_directory":str(host),"overrides":{"provider":"fixture","model":"fixture-model","base_url":"https://provider.example/v1","api":"openai-completions","api_key_env":"FABRIC_TEST_SECRET","thinking":"medium","model_config":{"name":"Fixture","reasoning":True,"context_window":32000,"max_tokens":4000,"input":["text"],"cost":{"input":0,"output":0,"cache_read":0,"cache_write":0}}},"capabilities":[{"name":"design","description":"Native protocol fixture","version":"1","input_schema":{"type":"object"},"output_schema":{"type":"object"}}]})
    path = operator / "fabric.yaml"
    path.write_text(json.dumps(config))
    processes = []
    logs = []
    def spawn(args, environment):
        log = open(root/f"process-{len(processes)}.log","w+")
        logs.append(log); cleanup.callback(log.close)
        process = subprocess.Popen(args,env=environment,stdout=log,stderr=log)
        processes.append(process)
        def stop():
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
                try: process.wait(timeout=40)
                except subprocess.TimeoutExpired: process.kill(); process.wait(); raise
        cleanup.callback(stop)
        return process
    controls = spawn([str(REPO/"bin/factory-runtime"),"--root",str(host/"runtime"),"--local-database","--listen",f"127.0.0.1:{controls_port}","--public-url",controls_url,"--token-file",str(host/"token")],host_env)

    def request(url, token=None, body=None):
        headers = {"Content-Type":"application/json"}
        if token: headers["Authorization"]="Bearer "+token
        req = urllib.request.Request(url,data=json.dumps(body).encode() if body is not None else None,headers=headers)
        with urllib.request.urlopen(req,context=client_tls,timeout=45) as response:
            return json.load(response)
    deadline = time.monotonic()+180
    while True:
        assert controls.poll() is None, (root/"process-0.log").read_text()
        try:
            request(controls_url+"/health",host_token);break
        except (urllib.error.URLError,ConnectionError):
            assert time.monotonic()<deadline,"Controls startup timeout"
            time.sleep(.1)
    fabric_env = {**env,"FABRIC_LISTEN":f"127.0.0.1:{fabric_port}","FABRIC_API_TOKEN_FILE":str(operator/"token"),"FABRIC_TEST_SECRET":"operator-host"}
    fabric = spawn([str(BINARY),"serve",str(path),str(operator/"state")],fabric_env)
    def call(method,arguments):
        return json.loads(subprocess.check_output([str(BINARY),"call",fabric_url,method,json.dumps(arguments)],env=fabric_env,stderr=subprocess.PIPE,timeout=45))
    deadline=time.monotonic()+30
    while True:
        assert fabric.poll() is None,(fabric.returncode,(root/"process-1.log").read_text())
        try: call("catalog",{});break
        except subprocess.CalledProcessError:
            assert time.monotonic()<deadline,"Fabric startup timeout"
            time.sleep(.1)
    for route in ("/health","/comms","/workspaces/"+"0"*32):
        try: request(controls_url+route)
        except urllib.error.HTTPError as error: assert error.code==401
        else: raise AssertionError("unauthenticated Controls access")
    denied=request(fabric_url+"/rpc",body={"method":"attach","params":{"key":"intruder"}})
    assert "error" in denied,denied
    work=call("invoke",{"request_key":"host-environment","package":"utility","capability":"echo","input":{"_read_environment":"HOST_VALUE"}})
    result=call("wait",{"id":work["id"],"seconds":20})
    assert result["state"]=="completed",result
    assert result["output"]["execution_environment"]=="execution-host",result
    staged=host/"runtime/workspaces"/work["id"]
    assert (staged/"input.json").is_file()
    assert not (operator/"state/invocations"/work["id"]/"context.json").exists()
    context=json.loads((staged/"context.json").read_text())
    denied=request(fabric_url+"/rpc",context["connection"]["token"],{"method":"attach","params":{"key":"escalate"}})
    assert "error" in denied,denied
    assert "error" in request(fabric_url+"/rpc","wrong-credential",{"method":"attach","params":{"key":"escalate"}})
    review=call("invoke",{"request_key":"network-review","package":"reviewer","capability":"review","input":{}})
    reviewed=call("wait",{"id":review["id"],"seconds":20})
    assert reviewed["state"]=="completed",reviewed
    assert reviewed["output"]["specialist"]["finding"]=="reviewed",reviewed
    readiness=call("invoke",{"request_key":"shared-readiness","package":"readiness","capability":"assess","input":{"task":"Investigate gateway latency"}})
    ready=call("wait",{"id":readiness["id"],"seconds":20})
    assert ready["state"]=="completed",ready
    assert ready["output"]["data"]["response"]["answers"]["readiness"]["choice"]=="clarify",ready
    assert not (host/"runtime/workspaces"/readiness["id"]).exists()
    native=call("invoke",{"request_key":"shared-native","package":"architect","capability":"design","input":{"task":"Inspect","delegate":True}})
    native_result=call("wait",{"id":native["id"],"seconds":25})
    assert native_result["state"]=="completed",native_result
    assert native_result["output"]["child"]["finding"]=="delegated via assignment",native_result
    child=call("get",{"id":native_result["output"]["child_id"]})
    assert child["parent_id"]==native["id"],child
    subprocess.run(["node",str(REPO/"tests/mcp-client.mjs"),str(BINARY),fabric_url],env={**fabric_env,"FABRIC_TEST_CONTROLS":str(REPO.parent/"controls")},check=True,timeout=90)
    print("PASS: separate services over verified HTTPS, host-side staging/environment, peer delegation, auth isolation, official MCP interoperability")
