#!/usr/bin/env python3
"""Writes 60 days (this period and the one before) of made up Claude Code logs under HOME/.claude/projects for the usage
screenshot: only the fields tterm reads (usage numbers, no content)."""
import datetime, json, os, random, re, sys

home = sys.argv[1]
random.seed(7)
now = datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0)
models = ["claude-opus-5-5"] * 6 + ["claude-sonnet-5-5"] * 3 + ["claude-haiku-5-5"]
for project in ["terminal", "postino", "telaio"]:
    # Claude Code names the folder after the project's path, every other character a '-'.
    cwd = os.path.join(home, "dev", project)
    folder = os.path.join(home, ".claude", "projects", re.sub(r"[^A-Za-z0-9]", "-", cwd))
    os.makedirs(folder, exist_ok=True)
    for day in range(60):
        if random.random() < 0.25:
            continue
        start = now - datetime.timedelta(days=day, hours=random.randint(1, 8))
        session = f"{project}-{day}"
        lines = []
        for task in range(random.randint(1, 4)):
            t = start + datetime.timedelta(minutes=task * 25)
            lines.append({"type": "user", "timestamp": t.isoformat().replace("+00:00", "Z"),
                          "sessionId": session, "message": {"role": "user", "content": "x"}})
            for answer in range(random.randint(4, 12)):
                t2 = t + datetime.timedelta(minutes=answer)
                lines.append({"type": "assistant", "timestamp": t2.isoformat().replace("+00:00", "Z"),
                              "sessionId": session,
                              "message": {"id": f"{session}-{task}-{answer}", "model": random.choice(models),
                                          "usage": {"input_tokens": random.randint(200, 4000),
                                                    "cache_creation_input_tokens": random.randint(2000, 30000),
                                                    "cache_read_input_tokens": random.randint(20000, 200000),
                                                    "output_tokens": random.randint(300, 6000)}}})
        with open(os.path.join(folder, session + ".jsonl"), "w") as f:
            for line in lines:
                f.write(json.dumps(line, separators=(",", ":")) + "\n")
