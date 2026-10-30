# An OpenSSH server for the end-to-end tests (scripts/e2e_openssh.sh), on
# the distribution BASE names: each has its own OpenSSH.
ARG BASE=alpine:3.22
FROM ${BASE}
COPY install.sh /e2e-image/
RUN sh /e2e-image/install.sh
# After the packages, so a change here does not install them again.
COPY entrypoint.sh /e2e-image/
ENTRYPOINT ["sh", "/e2e-image/entrypoint.sh"]
