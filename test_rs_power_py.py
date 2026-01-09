import sys
import random
import reedsolo

def test_rs_robustness(n, k, ber, iterations=1000):
    rs = reedsolo.RSCodec(n - k)
    successes = 0
    total_errors_in_success = 0
    
    for _ in range(iterations):
        data = bytes(random.getrandbits(8) for _ in range(k))
        encoded = rs.encode(data)
        
        # Inject noise
        ba = bytearray(encoded)
        errors = 0
        for i in range(len(ba)):
            for bit in range(8):
                if random.random() < ber:
                    ba[i] ^= (1 << bit)
                    errors += 1
        
        try:
            decoded, msgecc, err_pos = rs.decode(ba)
            if decoded == data:
                successes += 1
                total_errors_in_success += len(err_pos)
        except:
            pass
            
    print(f"Python RS({n},{k}) at BER {ber}:")
    print(f"  Success Rate: {successes/iterations*100:.2f}%")
    if successes > 0:
        print(f"  Avg Corrected: {total_errors_in_success/successes:.2f}")

if __name__ == "__main__":
    test_rs_robustness(120, 100, 0.001)
    test_rs_robustness(120, 100, 0.005)
    test_rs_robustness(120, 100, 0.01)
