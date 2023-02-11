FROM ubuntu

RUN apt-get update && apt-get -y install pulseaudio alsa-utils darkice curl

RUN sed "s/^load-module module-console-kit/#load-module module-console-kit/" -i /etc/pulse/default.pa \
    && mkdir -p /audio \
    && chmod 0777 /audio \
    && useradd -u 1000 -m -d /home/user -s /bin/bash user \
    && usermod -aG audio user

COPY dist/entrypoint.sh /
RUN chmod -R +x /usr/local/bin/
RUN chmod 777 /tmp

ENTRYPOINT ["/bin/bash", "/entrypoint.sh"]
