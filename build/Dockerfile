FROM rust:bullseye

RUN apt-get update && apt-get upgrade -y && apt-get autoremove -y
# RUN apt-get install -y curl

# RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# deps for the build
RUN apt-get install -y libsystemd-dev
RUN apt-get install -y libpython3-dev

#WORKDIR /usr/src/log-ship
#COPY ../log-ship .

CMD ["cargo", "build", "--release"]


