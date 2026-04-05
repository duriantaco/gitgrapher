#!/usr/bin/env node

/**
 * GitGrapher CLI — thin TypeScript wrapper around the Rust core.
 *
 * Phase 1: Uses child_process to call the Rust binary directly.
 * Future: Will use napi-rs bindings for zero-overhead calls.
 */

import { Command } from 'commander';

const program = new Command();

program
  .name('gitgrapher')
  .description('Rust-powered code intelligence for AI agents')
  .version('0.1.0');

program
  .command('analyze')
  .description('Analyze a repository and build its knowledge graph')
  .argument('[path]', 'Path to repository', '.')
  .option('-v, --verbose', 'Enable verbose output')
  .action(async (path: string, options: { verbose?: boolean }) => {
    console.log(`Analyzing: ${path}`);
    console.log('(TypeScript CLI wrapper — Rust napi bindings coming soon)');
    // TODO: Call Rust via napi-rs
  });

program
  .command('mcp')
  .description('Start MCP server for AI agent integration')
  .action(async () => {
    console.log('MCP server starting...');
    // TODO: Implement MCP server using @modelcontextprotocol/sdk
  });

program
  .command('list')
  .description('List indexed repositories')
  .action(() => {
    console.log('No indexed repositories yet.');
  });

program.parse();
