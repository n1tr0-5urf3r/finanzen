# syntax=docker/dockerfile:1

FROM node:22-bookworm-slim AS frontend-build
WORKDIR /build/frontend

COPY frontend/package.json frontend/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm \
    npm ci --no-audit --no-fund

COPY frontend/ ./
RUN npm run build


FROM rust:1.98-bookworm AS backend-build
WORKDIR /build/backend

COPY backend/Cargo.toml backend/Cargo.lock ./
# migrations/ is needed at BUILD time: sqlx::migrate! embeds the SQL into the binary,
# so it is not required in the runtime image.
COPY backend/migrations ./migrations
COPY backend/src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/backend/target \
    cargo build --release --locked --bin finanzen \
    && cp target/release/finanzen /tmp/finanzen


FROM debian:bookworm-slim AS runtime
WORKDIR /app

# rustls means no OpenSSL; ca-certificates is still needed to verify KitchenOwl's
# certificate, and tzdata for Europe/Berlin.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl tzdata \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 finanzen \
    && useradd --system --uid 10001 --gid finanzen --home-dir /app \
               --shell /usr/sbin/nologin finanzen \
    && mkdir -p /app/data/receipts /app/frontend-dist \
    && chown -R finanzen:finanzen /app

COPY --from=backend-build  --chown=finanzen:finanzen /tmp/finanzen /usr/local/bin/finanzen
COPY --from=frontend-build --chown=finanzen:finanzen /build/frontend/dist /app/frontend-dist

ENV APP_BIND=0.0.0.0:3100 \
    APP_DATA_DIR=/app/data \
    APP_FRONTEND_DIR=/app/frontend-dist \
    APP_TIMEZONE=Europe/Berlin \
    TZ=Europe/Berlin

USER finanzen
VOLUME ["/app/data"]
EXPOSE 3100

# /ready proves the pool answers and the migrations ran; /health only proves the
# process is up.
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl --fail --silent --show-error http://127.0.0.1:3100/api/v1/ready >/dev/null || exit 1

ENTRYPOINT ["finanzen"]
