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

def run_explorer(args, task_queue, thread_id):
    """Runs instances of explore or simulate and yields results."""
    while True:
        try:
            task = task_queue.get_nowait()
        except Exception:
            break

        encoding, file_size = task
        
        if args.file:
            # Mode: sweep file sizes using simulate
            cmd = [
                "./target/release/simulate",
                "--limit", str(args.limit),
                "--file-size", str(file_size),
                "--encodings", encoding,
                "--ber", str(args.ber),
                "--format", "csv"
            ]
        else:
            # Mode: random encodings using explore
            # We use a trick: run explore once but specify the encoding if we had it.
            # However, old 'explore' mode generates its own encodings.
            # To keep it simple, if no encoding is provided, it's the old mode.
            cmd = [
                "./target/release/explore",
                "--limit", str(args.limit),
                "--file-size", str(file_size),
                "--nb-encodings", "1",
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
        
        stdout_content, _ = process.communicate()
        
        if process.returncode != 0:
            print(f"Explorer/Simulator in thread {thread_id} exited with code {process.returncode}", file=sys.stderr)
            task_queue.task_done()
            continue

        # Parse CSV output
        try:
            lines = [l.strip() for l in stdout_content.split("\n") if l.strip()]
            if len(lines) < 2:
                task_queue.task_done()
                continue
            
            header = lines[0]
            data_line = lines[1]
            
            row_df = pd.read_csv(io.StringIO(f"{header}\n{data_line}"))
            
            # Map simulate metrics to expected dashboard format
            if args.file:
                mapped_row = {
                    "Encodings": encoding,
                    "File Size": float(file_size),
                    "Eff (%)": float(row_df["Transmission Efficiency (%)"].iloc[0]),
                    "File Loss (%)": float(row_df["File Loss Rate (%)"].iloc[0]),
                    "PDU Loss (%)": float(row_df["Packet Loss Rate (%)"].iloc[0]),
                    "Air-BER": float(row_df["Bit Error Rate (on air)"].iloc[0])
                }
                row_df = pd.DataFrame([mapped_row])
            else:
                # Ensure numeric columns
                numeric_cols = ["File Size", "Eff (%)", "File Loss (%)", "PDU Loss (%)", "Air-BER"]
                for col in numeric_cols:
                    if col in row_df.columns:
                        row_df[col] = pd.to_numeric(row_df[col], errors='coerce')

            print(f"[Thread {thread_id}] Result: {row_df.to_dict('records')[0]}")
            sys.stdout.flush()
            
            with results_lock:
                global df_results
                df_results = pd.concat([df_results, row_df], ignore_index=True)
        except Exception as e:
            print(f"Error parsing line from thread {thread_id}: {e}", file=sys.stderr)
            
        task_queue.task_done()

    with results_lock:
        global finished_threads
        finished_threads += 1

def main():
    parser = argparse.ArgumentParser(description="Parallel HQFBP Encoding Explorer with Live Visualization")
    parser.add_argument("--n-thread", type=int, default=1, help="Number of parallel threads")
    parser.add_argument("--limit", type=int, default=1000, help="Simulations per encoding")
    parser.add_argument("--file-size", type=int, default=1024, help="File size in bytes (negative for [10, abs(N)])")
    parser.add_argument("--nb-encodings", type=int, default=100, help="Total encodings to test per thread")
    parser.add_argument("--ber", type=float, default=0.001, help="Bit Error Rate")
    parser.add_argument("--port", type=int, default=8050, help="Dash server port")
    parser.add_argument("--file", type=str, help="File containing encodings (one per line)")
    parser.add_argument("--step", type=int, default=100, help="Step size for file size sweep")
    
    args = parser.parse_args()

    # Always build to ensure metrics fixes are picked up
    print("Building/updating Rust binaries...")
    subprocess.run(["cargo", "build", "--release", "--bin", "explore"], check=True)
    subprocess.run(["cargo", "build", "--release", "--bin", "simulate"], check=True)

    # Prepare tasks
    import queue
    task_queue = queue.Queue()
    
    if args.file:
        if not os.path.exists(args.file):
            print(f"Error: file {args.file} not found.", file=sys.stderr)
            sys.exit(1)
            
        with open(args.file, "r") as f:
            encodings = [line.strip() for line in f if line.strip()]
            
        max_size = abs(args.file_size)
        sizes = list(range(10, max_size + 1, args.step))
        if not sizes or sizes[-1] != max_size:
            sizes.append(max_size)
            
        for encoding in encodings:
            for size in sizes:
                task_queue.put((encoding, size))
    else:
        for _ in range(args.nb_encodings):
            task_queue.put((None, args.file_size))

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

        # Stable colors for encodings
        if args.file and "Encodings" in local_df.columns:
            unique_encs = sorted(local_df["Encodings"].unique())
            colors = px.colors.qualitative.Plotly  # or any palette you prefer
            color_map = {enc: colors[i % len(colors)] for i, enc in enumerate(unique_encs)}
        else:
            color_map = None

        fig_eff = px.scatter(
            local_df,
            x="Eff (%)",
            y="File Loss (%)",
            color="Encodings" if args.file else "File Size",
            color_discrete_map=color_map,
            hover_data=local_df.columns,
            title=f"Efficiency vs. File Loss (Total: {count})",
            color_continuous_scale="Viridis" if not args.file else None
        )
        fig_eff.update_yaxes(autorange="reversed")
        
        fig_size = px.scatter(
            local_df,
            x="File Size",
            y="Eff (%)",
            color="Encodings" if args.file else "File Loss (%)",
            color_discrete_map=color_map,
            hover_data=local_df.columns,
            title="File Size vs. Efficiency",
            color_continuous_scale="Cividis" if not args.file else None
        )
        
        fig_loss = px.scatter(
            local_df,
            x="File Size",
            y="File Loss (%)",
            color="Encodings" if args.file else "Eff (%)",
            color_discrete_map=color_map,
            hover_data=local_df.columns,
            title="File Size vs. File Loss",
            color_continuous_scale="Plasma" if not args.file else None
        )
        fig_loss.update_yaxes(autorange="reversed")

        fig_eff_loss = px.scatter(
            local_df,
            x="File Size",
            y="Eff / Loss",
            color="Encodings" if args.file else "Eff (%)",
            color_discrete_map=color_map,
            hover_data=local_df.columns,
            title="File Size vs. Efficiency / Loss",
            color_continuous_scale="Viridis" if not args.file else None
        )
        
        status = f"Processed {count} tasks across {args.n_thread} threads."
        if finished >= args.n_thread:
            status = f"✅ ALL EXPLORATIONS FINISHED. Total: {count}."
            
        return fig_eff, fig_size, fig_loss, fig_eff_loss, status

    # Start explorers
    executor = concurrent.futures.ThreadPoolExecutor(max_workers=args.n_thread)
    for i in range(args.n_thread):
        executor.submit(run_explorer, args, task_queue, i)

    print(f"Starting Dash server on http://127.0.0.1:{args.port}")
    app.run(port=args.port, debug=False)

if __name__ == "__main__":
    main()
