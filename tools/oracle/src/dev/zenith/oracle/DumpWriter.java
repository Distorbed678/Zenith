// Zenith parity oracle — original source, MIT OR Apache-2.0.
package dev.zenith.oracle;

import java.io.BufferedOutputStream;
import java.io.ByteArrayOutputStream;
import java.io.DataOutputStream;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;

/** Writes the ZPD1 format (docs/specs/parity/dump-format.md). Big-endian; strings are u16 length + UTF-8. */
final class DumpWriter implements AutoCloseable {
    private final DataOutputStream out;

    DumpWriter(Path path, String source, String version, String scenario, long seed) throws IOException {
        this.out = new DataOutputStream(new BufferedOutputStream(Files.newOutputStream(path)));
        out.writeBytes("ZPD1");
        str(out, source);
        str(out, version);
        str(out, scenario);
        out.writeLong(seed);
    }

    RecordBuilder record(long tick, short phase, String subject) {
        return new RecordBuilder(tick, phase, subject);
    }

    private static void str(DataOutputStream o, String s) throws IOException {
        byte[] b = s.getBytes(StandardCharsets.UTF_8);
        if (b.length > 0xFFFF) {
            throw new IOException("string too long for ZPD1");
        }
        o.writeShort(b.length);
        o.write(b);
    }

    @Override
    public void close() throws IOException {
        out.close();
    }

    /** Builds one record body, then writes it length-prefixed. */
    final class RecordBuilder {
        private final ByteArrayOutputStream fieldBytes = new ByteArrayOutputStream();
        private final DataOutputStream fields = new DataOutputStream(fieldBytes);
        private final long tick;
        private final short phase;
        private final String subject;
        private int count;

        private RecordBuilder(long tick, short phase, String subject) {
            this.tick = tick;
            this.phase = phase;
            this.subject = subject;
        }

        RecordBuilder i64(String key, long v) throws IOException {
            str(fields, key);
            fields.writeByte(0);
            fields.writeLong(v);
            count++;
            return this;
        }

        RecordBuilder f64(String key, double v) throws IOException {
            str(fields, key);
            fields.writeByte(1);
            fields.writeLong(Double.doubleToRawLongBits(v));
            count++;
            return this;
        }

        RecordBuilder bool(String key, boolean v) throws IOException {
            str(fields, key);
            fields.writeByte(2);
            fields.writeByte(v ? 1 : 0);
            count++;
            return this;
        }

        RecordBuilder string(String key, String v) throws IOException {
            str(fields, key);
            fields.writeByte(3);
            str(fields, v);
            count++;
            return this;
        }

        void end() throws IOException {
            ByteArrayOutputStream bodyBytes = new ByteArrayOutputStream();
            DataOutputStream body = new DataOutputStream(bodyBytes);
            body.writeLong(tick);
            body.writeShort(phase);
            str(body, subject);
            body.writeShort(count);
            fields.flush();
            fieldBytes.writeTo(body);
            body.flush();
            out.writeInt(bodyBytes.size());
            bodyBytes.writeTo(out);
        }
    }
}
