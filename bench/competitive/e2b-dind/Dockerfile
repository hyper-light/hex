# The host E2B's stack runs on at adacachy (platform-rounds/e2b_adacachy.sh): Fedora 44 with Docker's engine and
# E2B's host prerequisites (iptables, rsync, e2fsprogs, iproute2; README.upstream.md), run privileged with a network
# namespace of its own. adacachy's UFW denies incoming traffic, which drops the hop from each E2B sandbox to its
# orchestrator's egress proxy on the host side of the sandbox's veth. Here that hop stays inside this container's
# namespace, and the stack's traffic out leaves through Docker's bridge NAT, so adacachy's firewall is left as it is.
# E2B's orchestrator is a glibc binary its launcher runs in PID 1's namespaces, hence Fedora, not docker:dind's Alpine.
FROM registry.fedoraproject.org/fedora:44
RUN dnf -y install --setopt=install_weak_deps=False moby-engine docker-compose iptables-nft iproute e2fsprogs rsync \
        kmod util-linux procps-ng python3 \
    && dnf clean all \
    && mkdir -p /etc/modules-load.d /etc/udev/rules.d /etc/sysctl.d
COPY entrypoint.sh /entrypoint.sh
ENTRYPOINT ["/bin/sh", "/entrypoint.sh"]
