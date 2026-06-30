#!/usr/bin/env node
/**
 * 将 CodeMirror 6 打成单文件 IIFE，供 Trunk 静态托管（`vendor/ide-codemirror.js`）。
 */
import * as esbuild from "esbuild";
import { mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..");
const entry = path.join(root, "scripts", "ide-codemirror-entry.mjs");
const outdir = path.join(root, "vendor");

await mkdir(outdir, { recursive: true });

await esbuild.build({
  entryPoints: [entry],
  bundle: true,
  format: "iife",
  globalName: "CrabMateIdeEditor",
  outfile: path.join(outdir, "ide-codemirror.js"),
  platform: "browser",
  target: ["es2020"],
  minify: true,
  legalComments: "none",
});

console.log("wrote vendor/ide-codemirror.js");
