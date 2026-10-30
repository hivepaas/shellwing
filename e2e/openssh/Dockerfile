# An OpenSSH server for the end-to-end tests (scripts/e2e_openssh.sh), on
# the distribution BASE names: each has its own OpenSSH.
ARG BASE=alpine:3.22
FROM ${BASE}
COPY install.sh entrypoint.sh /e2e-image/
RUN sh /e2e-image/install.sh
ENTRYPOINT ["sh", "/e2e-image/entrypoint.sh"]
