# voicevox-nemo-lite: drop-in replacement for voicevox/voicevox_nemo_engine that
# serves only POST /audio_query, POST /synthesis and GET /version.
#
# The VOICEVOX Nemo core (libvoicevox_core.so 0.15.0 + its bundled ONNX
# Runtime and encrypted models) and the OpenJTalk dictionary are copied from
# the official image, pinned by digest, so the bits are exactly what runs
# today. linux/amd64 only (the official image has no arm64 build).
#   docker build --platform linux/amd64 -t voicevox-nemo-lite .

ARG NEMO_IMAGE=voicevox/voicevox_nemo_engine@sha256:d3b897228de0128e9dffae53653b537ecb966aebae7c46b1a14e9104a1e22ad6

FROM ${NEMO_IMAGE} AS nemo

FROM rust:1-bookworm AS build
COPY --from=nemo /opt/voicevox_core/libvoicevox_core.so /opt/voicevox_core/libonnxruntime.so.1.13.1 /opt/voicevox_core/
ENV VOICEVOX_CORE_DIR=/opt/voicevox_core
WORKDIR /src
COPY Cargo.toml Cargo.lock* build.rs ./
COPY src ./src
RUN cargo build --release --locked || cargo build --release

FROM debian:bookworm-slim
# libvoicevox_core.so needs only glibc, libgcc_s and libstdc++ (via ONNX Runtime).
RUN apt-get update && apt-get install -y --no-install-recommends libstdc++6 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home voicevox
COPY --from=nemo /opt/voicevox_core/libvoicevox_core.so /opt/voicevox_core/libonnxruntime.so.1.13.1 /opt/voicevox_core/
COPY --from=nemo /opt/voicevox_core/model /opt/voicevox_core/model
COPY --from=nemo /opt/voicevox_core/README.txt /opt/voicevox_core/TERMS.txt
COPY --from=nemo /home/user/.local/lib/python3.11/site-packages/pyopenjtalk/open_jtalk_dic_utf_8-1.11 /opt/open_jtalk_dic_utf_8-1.11
COPY --from=build /src/target/release/voicevox-lite /usr/local/bin/voicevox-lite
# The core looks for model/ relative to the .so's directory.
WORKDIR /opt/voicevox_core
USER voicevox
ENV PORT=50121 CPU_NUM_THREADS=1 SPEAKERS=10006 \
    OPEN_JTALK_DICT_DIR=/opt/open_jtalk_dic_utf_8-1.11
EXPOSE 50121
CMD ["voicevox-lite"]
