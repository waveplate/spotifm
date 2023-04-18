FROM rust:1.62

RUN apt update && apt install -y libasound2-dev pkg-config libpulse-dev build-essential

COPY build.sh /tmp/build.sh
RUN chmod +x /tmp/build.sh

RUN useradd -m -u 1000 builder

ENTRYPOINT ["/tmp/build.sh"]
