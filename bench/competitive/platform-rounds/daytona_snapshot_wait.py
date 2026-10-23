import json, sys, time, urllib.request
key = open("/home/adalundhe/.cache/hex-bench/daytona.key").read().strip()
deadline = time.monotonic() + 1200
last = None
while True:
    req = urllib.request.Request("http://localhost:3000/api/snapshots?limit=20", headers={"Authorization": "Bearer " + key})
    try:
        items = json.load(urllib.request.urlopen(req, timeout=20)).get("items", [])
        states = {i.get("name"): i.get("state") for i in items if i.get("general")}
    except Exception as e:
        states = {"error": type(e).__name__}
    if states != last:
        print(time.strftime("%H:%M:%S"), states, flush=True); last = states
    if states and all(v == "active" for v in states.values()):
        print("SNAPSHOT ACTIVE"); sys.exit(0)
    if any(v in ("error", "build_failed") for v in states.values()):
        print("SNAPSHOT FAILED"); sys.exit(2)
    if time.monotonic() > deadline:
        print("TIMEOUT: snapshot not active after 20 min"); sys.exit(3)
    time.sleep(10)
