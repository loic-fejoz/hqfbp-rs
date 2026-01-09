import sys
import random

FEND = 0xC0
FESC = 0xDB
TFEND = 0xDC
TFESC = 0xDD

def inject_noise(data, ber):
    ba = bytearray(data)
    errors = 0
    for i in range(len(ba)):
        for bit in range(8):
            if random.random() < ber:
                ba[i] ^= (1 << bit)
                errors += 1
    return bytes(ba), errors

def kiss_unescape(frame):
    out = bytearray()
    escaped = False
    for b in frame:
        if escaped:
            if b == TFEND:
                out.append(FEND)
            elif b == TFESC:
                out.append(FESC)
            else:
                out.append(b)
            escaped = False
        elif b == FESC:
            escaped = True
        else:
            out.append(b)
    return bytes(out)

def kiss_escape(data):
    out = bytearray()
    for b in data:
        if b == FEND:
            out.append(FESC)
            out.append(TFEND)
        elif b == FESC:
            out.append(FESC)
            out.append(TFESC)
        else:
            out.append(b)
    return bytes(out)

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: inject_noise.py <input_file> <ber>")
        sys.exit(1)
    
    with open(sys.argv[1], "rb") as f:
        data = f.read()
    
    ber = float(sys.argv[2])
    
    # Simple KISS splitter
    frames = []
    current_frame = bytearray()
    in_frame = False
    
    for b in data:
        if b == FEND:
            if in_frame:
                if current_frame:
                    frames.append(bytes(current_frame))
                current_frame = bytearray()
                in_frame = False
            else:
                in_frame = True
                current_frame = bytearray()
        elif in_frame:
            current_frame.append(b)
            
    total_errors = 0
    sys.stdout.buffer.write(bytes([FEND]))
    for frame in frames:
        # KISS frame: [Cmd][Data...]
        # We inject noise into everything except the command byte? 
        # Actually, let's inject into the whole unescaped PDU but maybe keep Cmd if we want to be safe.
        # But real noise affects everything.
        unescaped = kiss_unescape(frame)
        noisy_unescaped, err_count = inject_noise(unescaped, ber)
        total_errors += err_count
        
        sys.stdout.buffer.write(bytes([0x00])) # Assume command 0
        sys.stdout.buffer.write(kiss_escape(noisy_unescaped[1:] if len(noisy_unescaped) > 0 else b""))
        sys.stdout.buffer.write(bytes([FEND]))
        
    print(f"DEBUG: Injected {total_errors} errors into {len(frames)} frames", file=sys.stderr)
