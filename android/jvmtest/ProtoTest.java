import dev.dropcopy.Proto;
import java.io.ByteArrayInputStream;
import java.io.FileInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Collections;
import java.util.List;

/**
 * Drives the Android app's protocol code against a running PC receiver.
 *
 *   javac -d out ../app/src/main/java/dev/dropcopy/Proto.java ProtoTest.java
 *   java -cp out ProtoTest "<pairing link>" FILE...
 *
 * Runs four cases: direct connect to the address in the link, discovery with no address,
 * a stale address that discovery must recover from, and a wrong key that must be refused.
 */
public class ProtoTest {
    static Proto.Item file(Path p) throws IOException {
        long size = Files.size(p);
        String name = p.getFileName().toString();
        return new Proto.Item() {
            public String name() { return name; }
            public String mime() { return name.endsWith(".png") ? "image/png" : "image/jpeg"; }
            public long size() { return size; }
            public InputStream open() throws IOException { return new FileInputStream(p.toFile()); }
        };
    }

    static Proto.Item unknownSize(String name, byte[] data) {
        return new Proto.Item() {
            public String name() { return name; }
            public String mime() { return "application/octet-stream"; }
            public long size() { return -1; }
            public InputStream open() { return new ByteArrayInputStream(data); }
        };
    }

    static int failures = 0;

    static void check(String what, boolean ok) {
        System.out.println((ok ? "  PASS " : "  FAIL ") + what);
        if (!ok) failures++;
    }

    public static void main(String[] args) throws Exception {
        Proto.Pc pc = Proto.Pc.parse(args[0]);
        List<Proto.Item> items = new ArrayList<>();
        for (int i = 1; i < args.length; i++) items.add(file(Path.of(args[i])));
        items.add(unknownSize("stream.bin", new byte[700_001]));
        items.add(Proto.text("hello from the phone code"));

        check("pairing link round-trips", Arrays.equals(Proto.Pc.parse(pc.toLink()).key, pc.key)
                && Proto.Pc.parse(pc.toLink()).name.equals(pc.name));

        // 1. direct connect to the address in the link
        Proto.Result r = Proto.send(List.of(pc), Proto.broadcastAddresses(), items, null);
        System.out.printf("    direct: %d bytes to %s, connect %d ms, total %d ms%n", r.bytes, r.host, r.connectMillis, r.totalMillis);
        check("direct send", r.bytes > 0);

        // 2. no known address: broadcast discovery only
        Proto.Pc fresh = new Proto.Pc(pc.id, pc.key, pc.name, null, pc.port);
        r = Proto.send(List.of(fresh), Proto.broadcastAddresses(), List.of(Proto.text("via discovery")), null);
        System.out.printf("    discovery: found %s:%d, connect %d ms%n", r.host, fresh.port, r.connectMillis);
        check("discovery send", r.host != null);

        // 3. stale address (PC moved): discovery must win the race
        Proto.Pc stale = new Proto.Pc(pc.id, pc.key, pc.name, "10.255.255.1", pc.port);
        r = Proto.send(List.of(stale), Proto.broadcastAddresses(), List.of(Proto.text("after the PC moved")), null);
        System.out.printf("    stale address: recovered via %s, connect %d ms%n", r.host, r.connectMillis);
        check("stale address recovers and is updated", !"10.255.255.1".equals(stale.host));

        // 4. wrong key
        byte[] badKey = pc.key.clone();
        badKey[0] ^= 1;
        Proto.Pc wrong = new Proto.Pc(pc.id, badKey, pc.name, pc.host, pc.port);
        try {
            Proto.send(List.of(wrong), Collections.emptyList(), List.of(Proto.text("x")), null);
            check("wrong key refused", false);
        } catch (IOException e) {
            check("wrong key refused (" + e.getMessage() + ")", true);
        }

        System.out.println(failures == 0 ? "ALL PASSED" : failures + " FAILED");
        System.exit(failures == 0 ? 0 : 1);
    }
}
