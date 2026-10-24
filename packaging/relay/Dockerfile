# borg-relay, the relay borgs meet through for borg cloud. By default it runs behind a proxy
# that adds TLS, such as Dokploy's or Coolify's. The release builds this image from the
# static binaries it already made, one per architecture, in amd64/ and arm64/, beside an
# empty data/ folder.
FROM alpine:3.22

ARG TARGETARCH
COPY ${TARGETARCH}/borg-relay /usr/local/bin/borg-relay
# Where the certificate is kept when the relay serves TLS itself (--domain ... --cache /data),
# writable by the user it runs as.
COPY --chown=65534:65534 --chmod=700 data /data

LABEL org.opencontainers.image.title="borg-relay" \
      org.opencontainers.image.description="The relay borgs meet through for borg cloud" \
      org.opencontainers.image.source="https://github.com/0xcd21h/borg-commander" \
      org.opencontainers.image.licenses="MIT OR Apache-2.0"

USER 65534:65534
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD ["borg-relay", "--health", "127.0.0.1:8080"]
ENTRYPOINT ["borg-relay"]
CMD ["--behind-proxy", "--listen", "0.0.0.0:8080"]
