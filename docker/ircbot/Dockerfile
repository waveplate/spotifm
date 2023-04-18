FROM node:latest

COPY index.js /home/node/index.js
COPY package.json /home/node/package.json

RUN cd /home/node && npm install

ENTRYPOINT ["node", "/home/node/index.js"]
