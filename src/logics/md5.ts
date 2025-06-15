/**
 * MD5 Hashing Algorithm Implementation.
 *
 * Based on RFC 1321.
 */
export class MD5 {
    /**
     * Shift constants (S_ij)
     * 
     * S[round_index][operation_index_in_pattern_of_4]
     */
    static S: number[][] = [
        [7, 12, 17, 22], // Round 1
        [5, 9, 14, 20],  // Round 2
        [4, 11, 16, 23], // Round 3
        [6, 10, 15, 21]  // Round 4
    ];
    /**
     * Constants K[i] for MD5 transformation.
     * 
     * K[i] = floor(abs(sin(i + 1)) * 2^32)
     */
    static K: number[] = (() => {
        const k = [];
        for (let i = 0; i < 64; i++) {
            k[i] = Math.floor(Math.abs(Math.sin(i + 1)) * (2 ** 32));
        }
        return k;
    })();

    /** Left rotate a 32-bit value by n bits. */
    static _rotl(x: number, n: number): number {
        return (x << n) | (x >>> (32 - n));
    }

    /** Add two 32-bit values with wrapping (modulo 2^32). */
    static _add(x: number, y: number): number {
        return (x + y) & 0xFFFFFFFF;
    }

    // MD5 basic logical functions
    static _F(x: number, y: number, z: number): number { return (x & y) | (~x & z); }
    static _G(x: number, y: number, z: number): number { return (x & z) | (y & ~z); }
    static _H(x: number, y: number, z: number): number { return x ^ y ^ z; }
    static _I(x: number, y: number, z: number): number { return y ^ (x | ~z); }

    private _state: number[];
    private _buffer: Uint8Array;
    private _bufferLength: number;
    private _totalLength: bigint;
    private _finalized: boolean;

    /**
     * @param dataChunk - Optional initial data chunk to process.
     */
    constructor(dataChunk: Uint8Array | null = null) {
        // Initial state variables (A, B, C, D)
        this._state = [
            0x67452301,
            0xefcdab89,
            0x98badcfe,
            0x10325476
        ];

        // Buffer for unprocessed data (max 64 bytes)
        this._buffer = new Uint8Array(64);
        this._bufferLength = 0; // Number of bytes currently in the buffer

        // Total message length in bits (using BigInt for potentially large messages)
        this._totalLength = 0n;

        // Flag to ensure _finalize is called only once
        this._finalized = false;

        // If an initial data chunk is provided, process it
        if (dataChunk != null) {
            this.update(dataChunk);
        }
    }

    /**
     * Core MD5 transformation function for a 64-byte block.
     * Processes the block and updates the internal state.
     * @param block - A 64-byte block of data to process.
     */
    private _transform(block: Uint8Array): void {
        let a = this._state[0];
        let b = this._state[1];
        let c = this._state[2];
        let d = this._state[3];

        const M = new Uint32Array(16); // 16 32-bit words (little-endian)
        for (let i = 0; i < 16; i++) {
            const j = i * 4;
            M[i] = (block[j]) | (block[j + 1] << 8) | (block[j + 2] << 16) | (block[j + 3] << 24);
        }

        // Main loop: 4 rounds, 16 operations each
        for (let i = 0; i < 64; i++) {
            let f, g, round;
            if (i < 16) {        // Round 1
                f = MD5._F(b, c, d);
                g = i;
                round = 0;
            } else if (i < 32) { // Round 2
                f = MD5._G(b, c, d);
                g = (5 * i + 1) % 16;
                round = 1;
            } else if (i < 48) { // Round 3
                f = MD5._H(b, c, d);
                g = (3 * i + 5) % 16;
                round = 2;
            } else {             // Round 4
                f = MD5._I(b, c, d);
                g = (7 * i) % 16;
                round = 3;
            }

            const shift = MD5.S[round][i % 4];
            const k_i = MD5.K[i];

            let temp_sum = MD5._add(a, f);
            temp_sum = MD5._add(temp_sum, k_i);
            temp_sum = MD5._add(temp_sum, M[g]);

            const temp_d = d;
            d = c;
            c = b;
            b = MD5._add(b, MD5._rotl(temp_sum, shift));
            a = temp_d;
        }

        this._state[0] = MD5._add(this._state[0], a);
        this._state[1] = MD5._add(this._state[1], b);
        this._state[2] = MD5._add(this._state[2], c);
        this._state[3] = MD5._add(this._state[3], d);
    }

    /**
     * Updates the hash computation with a new chunk of data.
     * @param dataChunk - The chunk of data to process.
     */
    update(dataChunk: Uint8Array): void {
        if (!(dataChunk instanceof Uint8Array)) {
            throw new TypeError('Expects a Uint8Array as input.');
        }

        if (this._finalized) {
            throw new Error('MD5: Cannot update after finalizing. Create a new instance for a new hash.');
        }

        const dataLen = dataChunk.length;
        this._totalLength += BigInt(dataLen) * 8n; // Update total length in bits

        let offset = 0;

        // Fill buffer if it has pending data
        if (this._bufferLength > 0) {
            const spaceLeft = 64 - this._bufferLength;
            const toCopy = Math.min(dataLen, spaceLeft);
            this._buffer.set(dataChunk.subarray(0, toCopy), this._bufferLength);
            this._bufferLength += toCopy;
            offset += toCopy;

            if (this._bufferLength === 64) {
                this._transform(this._buffer);
                this._bufferLength = 0;
            }
        }

        // Process full 64-byte blocks from the remaining dataChunk
        while (offset + 64 <= dataLen) {
            this._transform(dataChunk.subarray(offset, offset + 64));
            offset += 64;
        }

        // Store any remaining part of dataChunk in the buffer
        if (offset < dataLen) {
            const remaining = dataLen - offset;
            this._buffer.set(dataChunk.subarray(offset), 0); // Store at the beginning
            this._bufferLength = remaining;
        }
    }

    /**
     * Finalizes the hash computation by applying padding and processing the final block.
     * This method should be called once after all data has been updated.
     */
    private _finalize(): void {
        if (this._finalized) {
            return; // Already finalized, do nothing
        }

        // Append padding bit (0x80)
        this._buffer[this._bufferLength++] = 0x80;

        // If buffer became full after 0x80, transform and clear
        if (this._bufferLength === 64) {
            this._transform(this._buffer);
            this._bufferLength = 0;
        }

        // Pad with zeros until buffer length is 56 (mod 64)
        // (i.e., until 64 - this._bufferLength === 8)
        while (this._bufferLength % 64 !== 56) {
            this._buffer[this._bufferLength++] = 0;
            // If buffer becomes full while padding zeros, transform and clear
            if (this._bufferLength === 64) {
                this._transform(this._buffer);
                this._bufferLength = 0;
            }
        }
        // At this point, this._bufferLength is 56.

        // Append original message length (64-bit little-endian)
        const L = this._totalLength; // L is a BigInt

        // Low 32 bits of length
        this._buffer[56] = Number(L & 0xFFn);
        this._buffer[57] = Number((L >> 8n) & 0xFFn);
        this._buffer[58] = Number((L >> 16n) & 0xFFn);
        this._buffer[59] = Number((L >> 24n) & 0xFFn);

        // High 32 bits of length
        const L_high = L >> 32n;
        this._buffer[60] = Number(L_high & 0xFFn);
        this._buffer[61] = Number((L_high >> 8n) & 0xFFn);
        this._buffer[62] = Number((L_high >> 16n) & 0xFFn);
        this._buffer[63] = Number((L_high >> 24n) & 0xFFn);

        this._transform(this._buffer);
        this._finalized = true; // Mark as finalized
    }

    /**
     * Returns the MD5 hash as a Uint8Array.
     * @returns The 16-byte MD5 hash.
     */
    digest(): Uint8Array {
        this._finalize(); // Apply padding and final transform (idempotent)

        const hashBytes = new Uint8Array(16);
        for (let i = 0; i < 4; i++) {
            const val = this._state[i];
            hashBytes[i * 4 + 0] = (val & 0xFF);
            hashBytes[i * 4 + 1] = ((val >>> 8) & 0xFF);
            hashBytes[i * 4 + 2] = ((val >>> 16) & 0xFF);
            hashBytes[i * 4 + 3] = ((val >>> 24) & 0xFF);
        }
        return hashBytes;
    }

    /**
     * Returns the MD5 hash as a hexadecimal string.
     * @returns The MD5 hash as a 32-character hex string.
     */
    hexdigest(): string {
        const hashBytes = this.digest(); // digest() now handles finalization idempotently
        return Array.from(hashBytes)
            .map(b => b.toString(16).padStart(2, '0'))
            .join('');
    }
}
