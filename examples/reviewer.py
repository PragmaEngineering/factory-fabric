"""Deterministic reviewer fixture: delegates and communicates as its own peer."""
import json
import os
import subprocess
import sys
import time
import uuid
from echo import call


def main():
    with open(os.environ["FABRIC_CONTEXT_PATH"]) as source:
        context = json.load(source)
    with open(os.environ["FACTORY_INPUTS_PATH"]) as source:
        task = json.load(source)
    binary = sys.argv[1]
    socket = context.get("fabric_endpoint", context.get("fabric_socket"))

    def fabric(method, arguments):
        result = subprocess.run([binary, "call", socket, method, json.dumps(arguments)],
                                capture_output=True, text=True, check=False)
        if result.returncode:
            raise RuntimeError(result.stderr)
        return json.loads(result.stdout)

    try:
        child = fabric("invoke", {"request_key": "specialist", "package": "utility",
                                "capability": "echo", "input": {"_delay": 0.8, "finding": "reviewed"}})
    except RuntimeError as error:
        expected = task.get("expect_error")
        if expected and expected in str(error):
            output = {"handled_error": expected}
        else:
            raise
    else:
        deadline = time.monotonic() + 10
        while True:
            child = fabric("get", {"id": child["id"]})
            if child.get("running"):
                break
            if time.monotonic() >= deadline:
                raise RuntimeError("specialist did not start")
            time.sleep(0.03)
        running = child["running"]
        fabric("comms_call", {"name": "send_message", "arguments": {
            "operation_id": str(uuid.uuid4()), "thread_id": running["thread_id"],
            "recipients": [running["worker_id"]],
            "payload": {"kind": "question", "version": 1, "parts": [{"kind": "text", "text": "Please inspect the context and reply."}]},
        }})
        completed = fabric("wait", {"id": child["id"], "seconds": 10})
        if completed["state"] != "completed":
            raise RuntimeError("specialist failed: " + str(completed.get("error")))
        conversation = fabric("comms_call", {"name": "read_messages", "arguments": {"thread_id": running["thread_id"]}})
        answers = [m for m in conversation["messages"] if m["payload"]["kind"] == "answer"]
        if not answers:
            raise RuntimeError("no direct specialist reply")
        output = {"specialist": completed["output"], "child_id": child["id"],
                  "peer_answer": answers[0]["payload"]["parts"][0]["text"]}

    connection = context["connection"]
    call(connection, "initialize", {"protocolVersion": "2025-03-26", "capabilities": {},
         "clientInfo": {"name": "fabric-reviewer", "version": "1"}})
    call(connection, "tools/call", {"name": "send_message", "arguments": {
        "operation_id": str(uuid.uuid4()), "thread_id": context["thread_id"],
        "recipients": [context["result_recipient"]],
        "payload": {"kind": "fabric.result", "version": 1, "parts": [{
            "kind": "data", "data": {"invocation_id": context["invocation_id"], "output": output},
        }]},
    }})


if __name__ == "__main__":
    main()
