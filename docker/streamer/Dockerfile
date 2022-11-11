FROM ubuntu
RUN apt-get update && apt-get -y install pulseaudio alsa-utils darkice icecast2 socat netcat

RUN sed "s/^load-module module-console-kit/#load-module module-console-kit/" -i /etc/pulse/default.pa \
    && sed "s/ENABLE=false/ENABLE=true/" -i /etc/default/icecast2 \
    && sed "s/hackme/hackme/g" -i /etc/icecast2/icecast.xml \
    && mkdir -p /audio \
    && chmod 0777 /audio \
    && useradd -u 1000 -m -d /home/user -s /bin/bash user \
    && usermod -aG audio user

USER root

COPY darkice.cfg /etc/darkice.cfg
#COPY dist/spotify-server.sh /usr/local/bin
#COPY dist/spotify-play.sh /usr/local/bin
#COPY dist/spotify-fm /usr/local/bin
COPY dist/entrypoint.sh /
COPY dist/start-server.sh /usr/local/bin
RUN chmod -R +x /usr/local/bin/

EXPOSE 8000
EXPOSE 1234

USER root

ENTRYPOINT ["/bin/bash", "/entrypoint.sh"]
