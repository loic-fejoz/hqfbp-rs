import argparse
import subprocess
import threading
import pandas as pd
import plotly.express as px
from dash import Dash, dcc, html, Input, Output
import concurrent.futures
import io
import time
import os
import sys

# Data storage
results_lock = threading.Lock()
df_results = pd.DataFrame()
finished_threads = 0

def run_explorer(args, thread_id):
    """Runs one instance of the explore binary and yields results."""
    cmd = [
        "./target/release/explore",
        "--limit", str(args.limit),
        "--file-size", str(args.file_size),
        "--nb-encodings", str(args.nb_encodings),
        "--ber", str(args.ber),
        "--format", "csv"
    ]
    
    print(f"[Thread {thread_id}] Starting {cmd}...")

    process = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE,
        stderr=open(f"explore_{thread_id}.log", "a"),
        text=True,
        bufsize=1
    )
    
    header = None
    for line in iter(process.stdout.readline, ""):
        line = line.strip()
        if not line:
            continue
        
        if header is None:
            header = line
            continue
            
        # Parse CSV line
        try:
            # Print immediately to stdout as requested
            print(f"[Thread {thread_id}] {line}")
            sys.stdout.flush()
            
            if line.startswith("Testing"):
                continue
            row_df = pd.read_csv(io.StringIO(f"{header}\n{line}"))
            # Convert numeric columns to ensure proper plotting
            numeric_cols = ["File Size", "Eff (%)", "File Loss (%)", "PDU Loss (%)", "Air-BER"]
            for col in numeric_cols:
                if col in row_df.columns:
                    row_df[col] = pd.to_numeric(row_df[col], errors='coerce')
            
            with results_lock:
                global df_results
                df_results = pd.concat([df_results, row_df], ignore_index=True)
        except Exception as e:
            print(f"Error parsing line from thread {thread_id}: {e}", file=sys.stderr)
            
    process.wait()
    with results_lock:
        global finished_threads
        finished_threads += 1
        
    if process.returncode != 0:
        print(f"Explorer in thread {thread_id} exited with code {process.returncode}", file=sys.stderr)

def main():
    parser = argparse.ArgumentParser(description="Parallel HQFBP Encoding Explorer with Live Visualization")
    parser.add_argument("--n-thread", type=int, default=1, help="Number of parallel threads")
    parser.add_argument("--limit", type=int, default=1000, help="Simulations per encoding")
    parser.add_argument("--file-size", type=int, default=1024, help="File size in bytes (negative for [10, abs(N)])")
    parser.add_argument("--nb-encodings", type=int, default=100, help="Total encodings to test per thread")
    parser.add_argument("--ber", type=float, default=0.001, help="Bit Error Rate")
    parser.add_argument("--port", type=int, default=8050, help="Dash server port")
    
    args = parser.parse_args()

    # Always build to ensure metrics fixes are picked up
    print("Building/updating Rust explore binary...")
    subprocess.run(["cargo", "build", "--release", "--bin", "explore"], check=True)

    # Start Dash app in a separate thread
    app = Dash(__name__)
    
    app.layout = html.Div([
        html.H1("HQFBP Encoding Explorer (Live)", style={'textAlign': 'center'}),
        dcc.Interval(id="interval-component", interval=2000, n_intervals=0),
        html.Div([
            html.Div([
                dcc.Graph(id="live-graph-eff", style={'width': '49%', 'display': 'inline-block'}),
                dcc.Graph(id="live-graph-size", style={'width': '49%', 'display': 'inline-block'}),
            ]),
            html.Div([
                dcc.Graph(id="live-graph-loss", style={'width': '49%', 'display': 'inline-block'}),
                dcc.Graph(id="live-graph-eff-loss", style={'width': '49%', 'display': 'inline-block'}),
            ]),
        ]),
        html.Div(id="status-text", style={'fontSize': '20px', 'fontWeight': 'bold', 'textAlign': 'center', 'padding': '20px'}),
    ])

    @app.callback(
        [Output("live-graph-eff", "figure"), 
         Output("live-graph-size", "figure"), 
         Output("live-graph-loss", "figure"), 
         Output("live-graph-eff-loss", "figure"),
         Output("status-text", "children")],
        Input("interval-component", "n_intervals")
    )
    def update_graph(n):
        with results_lock:
            count = len(df_results)
            finished = finished_threads
            if df_results.empty:
                empty_fig = px.scatter(title="Waiting for data...")
                return empty_fig, empty_fig, empty_fig, empty_fig, "No data yet."
            
            # Local copy for plotting
            local_df = df_results.copy()
            
        # Add derived metric: Eff / Loss
        # Use epsilon to avoid division by zero
        local_df["Eff / Loss"] = local_df["Eff (%)"] / (local_df["File Loss (%)"] + 0.001)

        fig_eff = px.scatter(
            local_df,
            x="Eff (%)",
            y="File Loss (%)",
            color="File Size",
            hover_data=local_df.columns,
            title=f"Efficiency vs. File Loss (Total: {count})",
            color_continuous_scale="Viridis"
        )
        fig_eff.update_yaxes(autorange="reversed")
        
        fig_size = px.scatter(
            local_df,
            x="File Size",
            y="Eff (%)",
            color="File Loss (%)",
            hover_data=local_df.columns,
            title="File Size vs. Efficiency",
            color_continuous_scale="Cividis"
        )
        
        fig_loss = px.scatter(
            local_df,
            x="File Size",
            y="File Loss (%)",
            color="Eff (%)",
            hover_data=local_df.columns,
            title="File Size vs. File Loss",
            color_continuous_scale="Plasma"
        )
        fig_loss.update_yaxes(autorange="reversed")

        fig_eff_loss = px.scatter(
            local_df,
            x="File Size",
            y="Eff / Loss",
            color="Eff (%)",
            hover_data=local_df.columns,
            title="File Size vs. Efficiency / Loss",
            color_continuous_scale="Viridis"
        )
        
        status = f"Processed {count} encodings across {args.n_thread} threads."
        if finished >= args.n_thread:
            status = f"✅ ALL EXPLORATIONS FINISHED. Total encodings: {count}."
            
        return fig_eff, fig_size, fig_loss, fig_eff_loss, status

    # Start explorers
    executor = concurrent.futures.ThreadPoolExecutor(max_workers=args.n_thread)
    for i in range(args.n_thread):
        executor.submit(run_explorer, args, i)

    print(f"Starting Dash server on http://127.0.0.1:{args.port}")
    app.run(port=args.port, debug=False)

if __name__ == "__main__":
    main()
