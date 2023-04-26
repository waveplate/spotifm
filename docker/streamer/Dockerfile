FROM ubuntu:20.04

RUN apt-get update && apt-get -y install pulseaudio alsa-utils darkice curl sox espeak

RUN sed "s/^load-module module-console-kit/#load-module module-console-kit/" -i /etc/pulse/default.pa \
    && mkdir -p /audio \
    && chmod 0777 /audio \
    && useradd -u 1000 -m -d /home/user -s /bin/bash user \
    && usermod -aG audio user

COPY entrypoint.sh /
COPY keepalive.sh /
RUN chmod +x /entrypoint.sh
RUN chmod +x /keepalive.sh
RUN chmod -R +x /usr/local/bin/
RUN chmod 777 /tmp

ENTRYPOINT ["/bin/bash", "/entrypoint.sh"]
