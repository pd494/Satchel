# satchel

An Effect-based [Spectrum](https://photon.codes/docs/spectrum-ts) iMessage app.

## Setup

Copy `.env.example` to `.env` and provide the Photon `PROJECT_ID` and
`PROJECT_SECRET`, then install dependencies:

```sh
bun install
```

## Run

```sh
bun start
```

`bun start` is the real-phone iMessage entry point. Credential-free integration
tests instantiate Spectrum with an in-process provider and exercise real
message wrapping, dispatch, error propagation, and provider shutdown:

```sh
bun run check
bun run test
```
