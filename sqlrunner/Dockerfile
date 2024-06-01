FROM huecker.io/library/rust:latest
RUN cargo install diesel_cli --no-default-features --features postgres
COPY . .
ENTRYPOINT ["diesel", "setup"]
