import os
import json
import subprocess
import sys
import hashlib
import shutil

def hash_file(path):
    h = hashlib.sha256()
    with open(path, 'rb') as f:
        while chunk := f.read(8192):
            h.update(chunk)
    return h.hexdigest()

def main():
    if len(sys.argv) < 2:
        print("Usage: python test_against_py_samples.py <samples_dir>")
        sys.exit(1)

    samples_dir = os.path.abspath(sys.argv[1])
    script_dir = os.path.dirname(os.path.abspath(__file__))
    project_root = os.path.dirname(script_dir)
    
    # Ensure rust binaries are built
    print("Building hqfbp-rs binaries...")
    subprocess.run(["cargo", "build", "--bin", "pack", "--bin", "unpack"], cwd=project_root, check=True)
    
    test_payload = os.path.join(samples_dir, "test_payload.bin")
    if not os.path.exists(test_payload):
        print(f"Error: {test_payload} not found")
        sys.exit(1)
    
    payload_size = os.path.getsize(test_payload)

    results = []
    
    json_files = [f for f in os.listdir(samples_dir) if f.endswith(".json")]
    json_files.sort()
    
    outputs_dir = os.path.join(project_root, "outputs")
    os.makedirs(outputs_dir, exist_ok=True)
    
    print(f"Comparing against samples in {samples_dir}...")

    for jf in json_files:
        with open(os.path.join(samples_dir, jf), 'r') as f:
            config = json.load(f)
        
        name = config['name']
        ref_kiss = os.path.join(samples_dir, config['output'])
        rs_kiss = os.path.join(outputs_dir, f"rs_{config['output']}")
        
        print(f"Testing {name}...")
        
        cmd = [
            "cargo", "run", "--quiet", "--bin", "pack", "--",
            test_payload,
            "--src-callsign", config['src_callsign'],
            "--output", rs_kiss
        ]
        
        if config.get('encodings'):
            cmd.extend(["--encodings", config['encodings']])
            
        if config.get('announcement_encodings'):
            cmd.extend(["--ann-encodings", config['announcement_encodings']])
            
        if config.get('max_payload_size'):
            cmd.extend(["--max-payload-size", str(config['max_payload_size'])])

        try:
            # Run pack
            subprocess.run(cmd, cwd=project_root, check=True, capture_output=True, text=True)
            
            ref_hash = hash_file(ref_kiss)
            rs_hash = hash_file(rs_kiss)
            
            match = ref_hash == rs_hash
            status = "PASS" if match else f"FAIL (Mismatch)"
            
            # Unpack and verify
            unpack_dir = os.path.join(outputs_dir, f"unpacked_{name}")
            if os.path.exists(unpack_dir):
                shutil.rmtree(unpack_dir)
            os.makedirs(unpack_dir, exist_ok=True)
            
            unpack_res = subprocess.run(
                ["cargo", "run", "--quiet", "--bin", "unpack", "--", unpack_dir, "--input", ref_kiss],
                cwd=project_root, capture_output=True, text=True
            )
            
            unpack_status = "FAILED"
            if unpack_res.returncode == 0:
                orig_hash = hash_file(test_payload)
                unpacked_match = False
                actual_payload_hash = None # To store the hash of the first file found, if any
                for f in os.listdir(unpack_dir):
                    current_unpacked_file_path = os.path.join(unpack_dir, f)
                    current_unpacked_hash = hash_file(current_unpacked_file_path)
                    if current_unpacked_hash == orig_hash:
                        unpacked_match = True
                        break
                    if actual_payload_hash is None: # Store the hash of the first file if no match yet
                        actual_payload_hash = current_unpacked_hash

                if unpacked_match:
                    unpack_status = "OK"
                else:
                    # If no file matched, and there was at least one file, report its hash
                    if actual_payload_hash:
                        print(f"DEBUG: {name} Unpack Hash Mismatch! Expected {orig_hash}, got {actual_payload_hash} (from first unpacked file)")
                    else:
                        print(f"DEBUG: {name} Unpack Hash Mismatch! Expected {orig_hash}, but no files found in {unpack_dir}")
                    unpack_status = "MISMATCH"
            else:
                last_line = unpack_res.stderr.strip().splitlines()[-1] if unpack_res.stderr else "unknown error"
                unpack_status = f"ERROR ({last_line})"

            results.append((name, status, unpack_status))
            print(f"  Result: {status}, Unpack: {unpack_status}")
            
        except subprocess.CalledProcessError as e:
            err_msg = e.stderr.strip().splitlines()[-1] if e.stderr else str(e)
            results.append((name, f"ERROR: {err_msg}", "N/A"))
            print(f"  Error: {err_msg}")

    print("\nSummary Comparison Matrix:")
    print(f"{'Test Case':<20} | {'Pack Identity':<15} | {'Unpack Status'}")
    print("-" * 55)
    for name, status, unpack_s in results:
        print(f"{name:<20} | {status:<15} | {unpack_s}")

if __name__ == "__main__":
    main()
