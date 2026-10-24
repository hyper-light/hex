# The libcrun driver's build (build.sh), from crun's pinned release tarball (fetch.py crun-src): Fedora 44, as
# the OCI pass's container and the bench host are, so the shared libraries it links (json-c, libcap,
# libseccomp) are the ones the pass's host /usr provides. host.sh builds it, copies the driver out to the
# competitors' bin directory, and records the image's id beside the results.
FROM registry.fedoraproject.org/fedora:44

RUN dnf -y install --setopt=install_weak_deps=False \
        gcc make findutils diffutils tar gzip python3 pkgconf-pkg-config json-c-devel libcap-devel libseccomp-devel \
    && dnf clean all

COPY crun.tar.gz driver.c build.sh /src/
RUN BUILD_DIR=/src bash /src/build.sh /src/crun.tar.gz /usr/local/bin/libcrun-driver
