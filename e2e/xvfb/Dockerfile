# An X server for the end-to-end tests (scripts/e2e_openssh.sh), on a TCP port,
# with a cookie made for the run and the SECURITY extension limited cookies need.
FROM alpine:3.22
RUN apk add --no-cache xvfb xauth xdpyinfo
COPY entrypoint.sh /e2e-image/
ENTRYPOINT ["sh", "/e2e-image/entrypoint.sh"]
