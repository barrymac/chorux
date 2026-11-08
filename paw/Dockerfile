# chorux paw — the browser door (docs/08). Zero-dep node adapter; the whole
# image is a node runtime plus two files. The paw never sees payloads (L2).
FROM node:22-alpine
WORKDIR /paw
COPY server.js paw.html ./
ENV PAW_PORT=8090
EXPOSE 8090
USER node
CMD ["node", "server.js"]
