import argparse
import asyncio
import json
import math
import statistics
import time

REPLY_LIMIT = 2 * 1024 * 1024


class Probe:
    def __init__(self, args):
        self.args = args
        self.process = None
        self.sequence = 0

    async def call(self, tool, arguments):
        started = time.monotonic()
        if self.process is None:
            self.process = await asyncio.create_subprocess_exec(
                self.args.borg, "mcp", "remote", self.args.server,
                stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.DEVNULL, limit=REPLY_LIMIT,
            )
        self.sequence += 1
        request = {
            "jsonrpc": "2.0", "id": self.sequence, "method": "tools/call",
            "params": {"name": tool, "arguments": arguments},
        }
        self.process.stdin.write((json.dumps(request) + "\n").encode())
        await self.process.stdin.drain()
        line = await self.process.stdout.readline()
        if not line:
            raise RuntimeError("Borg closed its diagnostic connection")
        result = json.loads(line)["result"]
        text = result["content"][0]["text"]
        if result.get("isError"):
            raise RuntimeError(text)
        return json.loads(text), round((time.monotonic() - started) * 1000, 1)

    async def close(self):
        if self.process is not None:
            if self.process.returncode is None:
                self.process.kill()
            await self.process.wait()
            self.process = None


def report(stage, **values):
    print(json.dumps({"stage": stage, **values}), flush=True)


async def measure(args):
    probe = Probe(args)
    stage = "connection_and_host_info"
    try:
        async def call(tool, arguments):
            return await asyncio.wait_for(probe.call(tool, arguments), args.timeout)

        host, elapsed = await call("host_info", {})
        report(stage, ms=elapsed, version=host.get("version"), service=host.get("service"))
        sessions = host.get("sessions", [])
        matches = [s for s in sessions if args.session in (s["id"], s["name"])]
        if len(matches) != 1:
            raise RuntimeError("Choose an unambiguous session name or ID from borg sessions list")
        session = matches[0]["id"]
        stage = "warm_host_info"
        samples = [(await call("host_info", {}))[1] for _ in range(args.samples)]
        report(stage, samples_ms=samples, median_ms=statistics.median(samples),
               p95_ms=sorted(samples)[math.ceil(len(samples) * 0.95) - 1])
        for size in args.sizes.split(","):
            cols, rows = map(int, size.split("x"))
            if not (1 <= cols <= 500 and 1 <= rows <= 200):
                raise ValueError("Frame size must be 1..500 columns by 1..200 rows")
            after = 0
            for kind in ("full_frame", "next_frame"):
                stage = kind
                frame, elapsed = await call("mirror_frame", {
                    "session": session, "after": after, "wait_ms": 0, "cols": cols, "rows": rows,
                })
                report(stage, requested_size=[cols, rows], ms=elapsed,
                       encoded_bytes=len(frame.get("data", "")), reset=frame.get("reset"),
                       native_input=frame.get("native_input", False))
                after = frame["sequence"]
    except (TimeoutError, RuntimeError, ValueError, KeyError) as error:
        report(stage, error=str(error) or "Response timed out", timeout_seconds=args.timeout)
        await probe.close()
        try:
            _, elapsed = await asyncio.wait_for(probe.call("host_info", {}), args.timeout)
            report("health_after_failure", ms=elapsed, reachable=True)
        except (TimeoutError, RuntimeError, ValueError, KeyError) as health_error:
            report("health_after_failure", reachable=False, error=str(health_error) or "Response timed out")
        return 1
    finally:
        await probe.close()
    return 0


def positive(value):
    result = int(value)
    if result < 1:
        raise argparse.ArgumentTypeError("Must be positive")
    return result


parser = argparse.ArgumentParser(description="Read-only Borg relay and screen latency measurements; no keys or screen contents are printed.")
parser.add_argument("server", help="Paired host name")
parser.add_argument("--session", default="default")
parser.add_argument("--borg", default="target/debug/borg")
parser.add_argument("--samples", type=positive, default=10)
parser.add_argument("--timeout", type=positive, default=15)
parser.add_argument("--sizes", default="20x5,150x60")
if __name__ == "__main__":
    raise SystemExit(asyncio.run(measure(parser.parse_args())))
