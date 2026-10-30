# A software security key for the end-to-end tests (scripts/e2e_openssh.sh):
# sk-dummy, which OpenSSH's own tests use in place of a hardware key. It is
# built from the source of the OpenSSH installed here, whose ssh-agent loads it.
FROM alpine:3.22
RUN apk add --no-cache openssh openssh-sk-helper build-base openssl-dev zlib-dev linux-headers curl
RUN version="$(ssh -V 2>&1 | sed -E 's/^OpenSSH_([^,]+),.*/\1/')" \
    && mkdir /tmp/src \
    && curl -fsSL "https://cdn.openbsd.org/pub/OpenBSD/OpenSSH/portable/openssh-${version}.tar.gz" \
        | tar xz -C /tmp/src --strip-components=1 \
    && cd /tmp/src \
    && ./configure >/dev/null \
    && make -j"$(nproc)" -C openbsd-compat >/dev/null \
    && make -j"$(nproc)" regress/misc/sk-dummy/sk-dummy.so >/dev/null \
    && cp regress/misc/sk-dummy/sk-dummy.so /usr/local/lib/sk-dummy.so \
    && rm -rf /tmp/src
COPY entrypoint.sh /e2e-image/
ENTRYPOINT ["sh", "/e2e-image/entrypoint.sh"]
