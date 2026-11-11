import dev.dropcopy.Proto;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.IOException;
import java.io.OutputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.TimeUnit;

/**
 * v2 against two real PC receivers on this machine: the phone's "which PC is in use?" question,
 * collecting what a PC sent to the phone (`dropcopy share`), and introducing the PCs to each other
 * so one sends to the other.
 *
 *   javac -d out ../app/src/main/java/dev/dropcopy/Proto.java V2Test.java
 *   java -cp out V2Test <dropcopy binary>
 */
public class V2Test {
    static int failures = 0;

    static void check(String what, boolean ok) {
        System.out.println((ok ? "  PASS " : "  FAIL ") + what);
        if (!ok) failures++;
    }

    static String run(String exe, Path home, String... args) throws Exception {
        List<String> cmd = new ArrayList<>(List.of(exe, "--home", home.toString()));
        cmd.addAll(List.of(args));
        Process p = new ProcessBuilder(cmd).redirectErrorStream(true).start();
        String out = new String(p.getInputStream().readAllBytes());
        p.waitFor(30, TimeUnit.SECONDS);
        return out;
    }

    static Process serve(String exe, Path home, int port, String idle) throws Exception {
        ProcessBuilder pb = new ProcessBuilder(exe, "--home", home.toString(), "--port", "" + port, "--clip", "off", "serve")
                .redirectErrorStream(true).redirectOutput(home.resolve("serve.log").toFile());
        pb.environment().put("DROPCOPY_IDLE", idle);
        return pb.start();
    }

    static String link(String exe, Path home, int port) throws Exception {
        String out = run(exe, home, "--port", "" + port, "pair");
        int i = out.indexOf("dropcopy://");
        return out.substring(i).split("\\s")[0].replaceAll("&a=[^&]*", "") + "&a=127.0.0.1";
    }

    public static void main(String[] args) throws Exception {
        String exe = args[0];
        Path root = Files.createTempDirectory("dropcopy-v2");
        Path deskHome = root.resolve("desk"), lapHome = root.resolve("laptop");
        Files.createDirectories(deskHome);
        Files.createDirectories(lapHome);
        run(exe, deskHome, "name", "Desk");
        run(exe, lapHome, "name", "Laptop");
        run(exe, deskHome, "dir", root.resolve("desk-in").toString());
        run(exe, lapHome, "dir", root.resolve("laptop-in").toString());
        // One machine has one discovery port: the desk answers status; the laptop is found by address.
        Process desk = serve(exe, deskHome, 48660, "3");
        Thread.sleep(400);
        Process lap = serve(exe, lapHome, 48661, "900");
        try {
            Thread.sleep(600);
            Proto.Pc d = Proto.Pc.parse(link(exe, deskHome, 48660));
            Proto.Pc l = Proto.Pc.parse(link(exe, lapHome, 48661));
            List<Proto.Pc> pcs = List.of(l, d); // the laptop was used last

            // 1. Which PC: the desk (touched 3 s ago) wins over the laptop used last.
            List<Proto.Status> answers = Proto.ask(pcs, Proto.broadcastAddresses(), List.of(Proto.DEFAULT_ROUTE), 400);
            Proto.Status pick = Proto.choose(pcs, answers);
            check("the desk answers status (" + answers.size() + " answer)", answers.size() >= 1);
            check("picks the PC in use, not the last used", pick != null && pick.pc == d && pick.idleSeconds == 3 && pick.port == 48660);
            check("no answers: no pick (falls back to the race)", Proto.choose(pcs, new ArrayList<>()) == null);

            // 2. A check-in tells the phone the PC speaks v2; with both, it introduces them.
            List<Proto.Item> hello = List.of(Proto.device("JVM Phone"), Proto.introductions(pcs));
            Proto.Result r = Proto.send(List.of(d), List.of(), hello, null);
            check("OK carries the v2 flag", (r.flags & Proto.OK_V2) != 0 && d.v2);
            check("an answer to the question marks it v2 too", d.v2);
            Proto.send(List.of(l), List.of(), hello, null);
            check("the laptop (no answer: port taken) learns it's v2 from its OK", l.v2);
            Proto.send(List.of(l), List.of(), hello, null); // now introduced
            String peers = Files.readString(deskHome.resolve("peers.txt"));
            check("desk learned the laptop as a peer, without its key", peers.contains("Laptop") && !peers.contains(java.util.Base64.getUrlEncoder().withoutPadding().encodeToString(l.key)));

            // 3. The desk sends to the laptop over the peer link (`dropcopy share --to`).
            Path note = root.resolve("note.txt");
            Files.writeString(note, "desk to laptop");
            String out = run(exe, deskHome, "share", "--to", "Laptop", note.toString());
            check("PC to PC: " + out.trim().replace('\n', ' '), Files.exists(root.resolve("laptop-in/note.txt")));

            // 4. The desk sends to the phone: it waits in the outbox until the phone collects.
            Path photo = root.resolve("photo.jpg");
            byte[] data = new byte[1_234_567];
            new java.util.Random(1).nextBytes(data);
            Files.write(photo, data);
            Process share = new ProcessBuilder(exe, "--home", deskHome.toString(), "share", "--to", "JVM Phone", photo.toString(), "--text", "and a link")
                    .redirectErrorStream(true).start();
            Thread.sleep(700);
            Proto.Result r2 = Proto.send(List.of(d), List.of(), List.of(Proto.device("JVM Phone")), null);
            check("a check-in says something is waiting", (r2.flags & Proto.OK_WAITING) != 0);
            ByteArrayOutputStream got = new ByteArrayOutputStream();
            String[] meta = new String[2];
            List<String> texts = new ArrayList<>();
            int n = Proto.fetch(List.of(d), List.of(), List.of(Proto.DEFAULT_ROUTE), "JVM Phone", null, new Proto.Sink() {
                public OutputStream begin(String name, String mime, long size) { meta[0] = name; meta[1] = mime; return got; }
                public void end() { }
                public void abort() { }
                public void text(String t) { texts.add(t); }
            });
            String shareOut = new String(share.getInputStream().readAllBytes()).trim();
            share.waitFor(20, TimeUnit.SECONDS);
            check("fetched the photo and the text (" + n + " items)", n == 2 && java.util.Arrays.equals(got.toByteArray(), data)
                    && "photo.jpg".equals(meta[0]) && "image/jpeg".equals(meta[1]) && texts.equals(List.of("and a link")));
            check("the PC side reports it delivered: " + shareOut.replace('\n', ' '), shareOut.contains("Sent to JVM Phone"));
            Proto.Result r3 = Proto.send(List.of(d), List.of(), List.of(Proto.device("JVM Phone")), null);
            check("nothing waiting afterwards", (r3.flags & Proto.OK_WAITING) == 0);
        } finally {
            desk.destroy();
            lap.destroy();
        }
        System.out.println(failures == 0 ? "ALL PASSED" : failures + " FAILED");
        System.exit(failures == 0 ? 0 : 1);
    }
}
