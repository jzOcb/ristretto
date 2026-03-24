import { useEffect, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { FitAddon } from '@xterm/addon-fit';
import { SearchAddon } from '@xterm/addon-search';
import { Terminal } from '@xterm/xterm';
import '@xterm/xterm/css/xterm.css';

interface RawTerminalProps {
  agentId: string;
  output: string;
}

export default function RawTerminal({ agentId, output }: RawTerminalProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const lastRenderedRef = useRef(0);

  useEffect(() => {
    if (!containerRef.current) {
      return;
    }

    const terminal = new Terminal({
      theme: {
        background: '#09090b',
        foreground: '#e4e4e7',
        cursor: '#8b5cf6',
        cursorAccent: '#09090b',
        selectionBackground: 'rgba(139, 92, 246, 0.25)',
      },
      fontFamily: 'JetBrains Mono, SFMono-Regular, monospace',
      fontSize: 12.5,
      convertEol: true,
      cursorBlink: true,
      scrollback: 10000,
    });
    const fitAddon = new FitAddon();
    const searchAddon = new SearchAddon();
    terminal.loadAddon(fitAddon);
    terminal.loadAddon(searchAddon);
    terminal.open(containerRef.current);
    fitAddon.fit();
    terminal.write(output);
    lastRenderedRef.current = output.length;

    terminal.onData((data) => {
      const bytes = Array.from(new TextEncoder().encode(data));
      void invoke('write_to_pty', { agentId, data: bytes });
    });

    const resizeObserver = new ResizeObserver(() => {
      fitAddon.fit();
      void invoke('resize_pty', {
        agentId,
        cols: terminal.cols,
        rows: terminal.rows,
      });
    });
    resizeObserver.observe(containerRef.current);

    terminalRef.current = terminal;
    fitAddonRef.current = fitAddon;

    return () => {
      resizeObserver.disconnect();
      terminal.dispose();
      terminalRef.current = null;
      fitAddonRef.current = null;
      lastRenderedRef.current = 0;
    };
  }, [agentId]);

  useEffect(() => {
    const terminal = terminalRef.current;
    if (!terminal) {
      return;
    }

    const nextChunk = output.slice(lastRenderedRef.current);
    if (nextChunk) {
      terminal.write(nextChunk);
      lastRenderedRef.current = output.length;
    }
  }, [output]);

  return <div className="h-full min-h-[420px] w-full rounded-lg bg-zinc-950 p-2.5" ref={containerRef} />;
}
