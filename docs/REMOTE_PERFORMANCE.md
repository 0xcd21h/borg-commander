# Remote performance

The local viewer and host use persistent encrypted connections through the relay. Relay round
trip time remains part of every request; the changes below remove avoidable work around it.

## TCP sockets

Borg enables `TCP_NODELAY` when opening each relay WebSocket, before TLS or the WebSocket
handshake. The relay enables it on every accepted TCP connection, before wrapping it in TLS
or reading HTTP. This disables Nagle's small-packet buffering on both relay legs. Encryption,
message framing, and the TCP/WebSocket protocol remain the same.

The setting takes effect on new connections after restarting the updated Borg or relay.
Each endpoint controls its own outgoing TCP traffic, so updating a client does not change
the sockets of an older host or relay. A TLS proxy in front of the relay controls another
pair of sockets; Borg cannot configure those sockets.

Actual-socket tests verify the flag on outbound Borg sockets and accepted relay sockets.
A development comparison with 25 ms of application-data transit delay in each direction
on each relay leg measured median mirror typing echo at 141 ms before and 140 ms after;
remote console echo measured 117 ms before and 118 ms after. Each typing run used twelve
samples. These differences do not demonstrate a material speedup. The proxy terminates TCP,
so this comparison does not reproduce delayed TCP acknowledgements across an Internet path.
`TCP_NODELAY` removes one potential source of buffering; network RTT and the existing
request/reply exchanges still contribute to latency.

A public-relay comparison changing only the client measured twelve warm status requests
at 175 ms median before and 168 ms after. A full 150×60 frame took 270 ms before and
272 ms after. The host and public relay were unchanged. These samples do not separate
the small status difference from route variation, and full-frame transfer did not improve.

## Directory listing

The previous endpoint reopened the directory for every 128-entry page. Skipping to an offset
also requested metadata for all skipped entries. That made metadata work grow quadratically
with directory size.

The current streaming endpoint retains a directory handle behind an opaque cursor, tied to
the requesting computer and path. It enumerates forward once, like successive `readdir`
operations on one open directory. Entries include their metadata and are published in bounded
chunks. Chunks are pushed over one request, so large folders do not need one relay round trip
per page. The first chunk holds at most 128 entries and subsequent chunks hold at most 512.
A chunk also yields
after 25 ms of scanning, even if it has not reached its entry limit. One blocked filesystem
syscall must still complete before the worker can yield. Disk operations run on blocking
workers, outside the UI loop and cursor-map mutex.

There are at most 64 idle cursors per Borg process. Requests prune cursors idle for 30 seconds.
Completion closes the handle. Navigation drops the client connection; the host stops pushing
when the disconnect reaches it. Network backpressure bounds queued output.
Older clients retain the offset protocol; updated hosts skip raw directory records before
requesting metadata, so they avoid repeatedly statting earlier entries too.

The spinner now includes the wait before the first batch, while an old folder may still be
visible. Switching to a different host clears the old source before streaming new entries.

### Validation

On a temporary directory containing 2,048 files, the previous implementation performed
17,423 metadata reads; the cursor performed 2,048. One local run measured approximately
95 ms versus 16 ms for this work. These measurements exclude network latency and are not
a promise of an equivalent end-to-end speedup.

The syscall simulation test injects a 2 ms delay into each metadata operation and verifies
that a time-limited chunk returns before collecting its full entry limit. The encrypted
end-to-end test pairs disposable Borg processes through a localhost relay and checks all
2,048 entries across multiple chunks, without duplicates. It also verifies host version,
OS, execution mode, and session metadata.

One encrypted localhost run returned the first chunk in approximately 16 ms and all 2,048
entries in five pushed chunks in 157 ms. This excludes pairing/setup and Internet relay latency.

```sh
cargo test -p borg-app --bin borg remote_fs::tests -- --nocapture
cargo test -p borg-app --test e2e remote_host_reports_its_version_and_streams_large_folders_without_duplicate_pages
```

## Mirror

The previous viewer waited 80 ms after each frame request. Its display future also stopped
being polled while a key awaited its network acknowledgement. A slow viewer could accumulate
and replay animation that was already out of date.

The display and input now run as separate tasks and connections. Input bursts are batched
within a bounded queue. Frame requests wait for new PTY output on the host, outside the
service loop and child mutex. The fixed 80 ms delay is gone. Older services remain usable
with a short idle backoff.

If an initial connection or a viewer is more than 64 KiB behind, the service sends a current
screen snapshot. This skips obsolete animation. Snapshots preserve the whole visible grid,
cursor and drawing attributes, including screens taller than 32 rows. Tests cover a 200 KB
animation backlog and a 60-row screen followed by incremental output.

Both the updated client and updated host are needed for long polling, snapshot catch-up,
and cursor listings. Older hosts remain compatible and show unavailable metadata explicitly.
