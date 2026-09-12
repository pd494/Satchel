import { Effect } from "effect";
import { runApp } from "../src";
import { sendMessage } from "../src/connection";

await Effect.runPromise(runApp(sendMessage));
