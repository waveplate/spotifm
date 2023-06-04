FROM node:latest

COPY index.js /home/node/index.js
COPY package.json /home/node/package.json

RUN cd /home/node && npm install

USER root

RUN apt update

RUN apt -y install ffmpeg

USER node

ENTRYPOINT ["node", "/home/node/index.js"]
