package fr.cydera.app;

import android.app.AlarmManager;
import android.app.PendingIntent;
import android.appwidget.AppWidgetManager;
import android.content.ComponentName;
import android.content.Context;
import android.content.Intent;
import android.os.SystemClock;
import android.view.View;
import android.widget.RemoteViews;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;
import java.text.SimpleDateFormat;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Calendar;
import java.util.List;
import java.util.Locale;

/**
 * Données et rendu des widgets Cydera. L'app (Rust) écrit {@code widget.json} et
 * {@code widget-edt.json} dans {@code files/data/cydera/} ; le widget n'y écrit jamais. Un appui
 * sur ▶/⏸ dépose une commande horodatée dans {@code widget-cmd/}, que l'app rejoue à son prochain
 * passage (voir {@code cydera_core::widget}). En attendant, l'affichage applique lui-même ces
 * commandes pour rester juste.
 */
final class WidgetData {
    static final String ACTION_TOGGLE = "fr.cydera.app.WIDGET_TOGGLE";
    static final String ACTION_REFRESH = "fr.cydera.app.WIDGET_REFRESH";

    private static final int TEXT = 0xFFF4F4F5;
    private static final int MUTED = 0xFFA1A1AA;
    private static final int ACCENT = 0xFF00D1FF;
    private static final int OK = 0xFF4ADE80;
    private static final int WARN = 0xFFEAB308;

    private WidgetData() {}

    static File dir(Context ctx) {
        return new File(ctx.getFilesDir(), "data/cydera");
    }

    private static JSONObject readJson(File f) {
        try (FileInputStream in = new FileInputStream(f)) {
            byte[] b = new byte[(int) f.length()];
            int n = 0;
            while (n < b.length) {
                int r = in.read(b, n, b.length - n);
                if (r < 0) break;
                n += r;
            }
            return new JSONObject(new String(b, 0, n, StandardCharsets.UTF_8));
        } catch (Exception e) {
            return null;
        }
    }

    private static long midnight(long ms) {
        Calendar c = Calendar.getInstance();
        c.setTimeInMillis(ms);
        c.set(Calendar.HOUR_OF_DAY, 0);
        c.set(Calendar.MINUTE, 0);
        c.set(Calendar.SECOND, 0);
        c.set(Calendar.MILLISECOND, 0);
        return c.getTimeInMillis();
    }

    private static String fmt(String pattern, long ms) {
        return new SimpleDateFormat(pattern, Locale.FRANCE).format(ms);
    }

    // ------------------------------------------------------------------ chrono

    static final class Chrono {
        String subject = "";
        String part = "";
        boolean running;
        long startMs;
        /** Temps enregistré aujourd'hui sur la matière, hors session en cours (ms). */
        long todayMs;
    }

    /** État de {@code widget.json}, commandes en attente appliquées. */
    static Chrono chrono(Context ctx) {
        Chrono c = new Chrono();
        long now = System.currentTimeMillis();
        JSONObject o = readJson(new File(dir(ctx), "widget.json"));
        if (o != null) {
            c.subject = o.optString("subject", "");
            c.part = o.optString("part", "");
            c.running = o.optBoolean("running", false);
            c.startMs = o.optLong("start_ms", 0);
            String today = fmt("yyyy-MM-dd", now);
            c.todayMs = today.equals(o.optString("day")) ? o.optLong("today_secs", 0) * 1000L : 0;
        }
        File[] cmds = new File(dir(ctx), "widget-cmd").listFiles();
        if (cmds != null) {
            Arrays.sort(cmds);
            for (File f : cmds) {
                String n = f.getName();
                int dot = n.indexOf('.');
                int dash = n.indexOf('-');
                if (dot < 0 || dash < 0) continue;
                long at;
                try {
                    at = Long.parseLong(n.substring(0, dash));
                } catch (NumberFormatException e) {
                    continue;
                }
                String act = n.substring(dot + 1);
                if (act.equals("start") && !c.running) {
                    c.running = true;
                    c.startMs = at;
                } else if (act.equals("pause") && c.running) {
                    if (at >= midnight(now)) c.todayMs += at - Math.max(c.startMs, midnight(now));
                    c.running = false;
                }
            }
        }
        return c;
    }

    /** Même format que le Chronometer : « 04:12 », « 1:02:30 ». */
    private static String hms(long ms) {
        long s = ms / 1000;
        if (s >= 3600) return String.format(Locale.ROOT, "%d:%02d:%02d", s / 3600, s % 3600 / 60, s % 60);
        return String.format(Locale.ROOT, "%02d:%02d", s / 60, s % 60);
    }

    private static int seq;

    /** Dépose un appui ▶/⏸ pour l'app (écriture atomique : fichier temporaire puis renommage). */
    static void command(Context ctx, String action) {
        File d = new File(dir(ctx), "widget-cmd");
        if (!d.isDirectory() && !d.mkdirs()) return;
        String name = String.format(Locale.ROOT, "%013d-%d", System.currentTimeMillis(), seq++);
        File tmp = new File(d, name + ".tmp");
        try (FileOutputStream out = new FileOutputStream(tmp)) {
            out.write(new byte[0]);
        } catch (Exception e) {
            return;
        }
        if (!tmp.renameTo(new File(d, name + "." + action))) tmp.delete();
    }

    static PendingIntent openApp(Context ctx, int code) {
        Intent i = ctx.getPackageManager().getLaunchIntentForPackage(ctx.getPackageName());
        if (i == null) i = new Intent();
        i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED);
        return PendingIntent.getActivity(ctx, code, i, PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
    }

    private static PendingIntent action(Context ctx, String action, int code) {
        Intent i = new Intent(ctx, WidgetActions.class).setAction(action);
        return PendingIntent.getBroadcast(ctx, code, i, PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
    }

    static void renderChrono(Context ctx) {
        AppWidgetManager m = AppWidgetManager.getInstance(ctx);
        int[] ids = m.getAppWidgetIds(new ComponentName(ctx, ChronoWidget.class));
        if (ids.length == 0) return;
        Chrono c = chrono(ctx);
        long now = System.currentTimeMillis();
        RemoteViews v = new RemoteViews(ctx.getPackageName(), R.layout.widget_chrono);
        boolean none = c.subject.isEmpty();
        v.setTextViewText(R.id.subject, none ? "Aucune matière" : c.subject + (c.part.isEmpty() ? "" : " · " + c.part));
        long live = c.running ? Math.max(0, now - Math.max(c.startMs, midnight(now))) : 0;
        long total = c.todayMs + live;
        if (c.running) {
            // Le Chronometer défile tout seul, sans réveiller l'app : base = horloge système − temps du jour.
            v.setChronometer(R.id.chrono, SystemClock.elapsedRealtime() - total, null, true);
            v.setTextColor(R.id.chrono, OK);
            v.setViewVisibility(R.id.chrono, View.VISIBLE);
            v.setViewVisibility(R.id.chrono_static, View.GONE);
        } else {
            v.setChronometer(R.id.chrono, SystemClock.elapsedRealtime(), null, false);
            v.setTextViewText(R.id.chrono_static, hms(total));
            v.setViewVisibility(R.id.chrono, View.GONE);
            v.setViewVisibility(R.id.chrono_static, View.VISIBLE);
        }
        String status;
        if (none) status = "Touche pour choisir une matière";
        else if (c.running) status = "aujourd'hui · en cours depuis " + fmt("HH:mm", c.startMs);
        else status = "aujourd'hui · en pause";
        v.setTextViewText(R.id.status, status);
        v.setTextColor(R.id.status, c.running ? OK : MUTED);
        v.setImageViewResource(R.id.toggle, c.running ? R.drawable.ic_pause : R.drawable.ic_play);
        v.setInt(R.id.toggle, "setColorFilter", c.running ? WARN : OK);
        v.setOnClickPendingIntent(R.id.body, openApp(ctx, 1));
        v.setOnClickPendingIntent(R.id.toggle, none ? openApp(ctx, 2) : action(ctx, ACTION_TOGGLE, 3));
        m.updateAppWidget(ids, v);
    }

    // ------------------------------------------------------------------ emploi du temps

    static final class Course {
        long start, end;
        String title, category, room, kind;
    }

    static List<Course> courses(Context ctx) {
        List<Course> out = new ArrayList<>();
        JSONObject o = readJson(new File(dir(ctx), "widget-edt.json"));
        JSONArray a = o == null ? null : o.optJSONArray("courses");
        if (a == null) return out;
        for (int i = 0; i < a.length(); i++) {
            JSONObject j = a.optJSONObject(i);
            if (j == null) continue;
            Course c = new Course();
            c.start = j.optLong("start_ms");
            c.end = j.optLong("end_ms");
            c.title = j.optString("title");
            c.category = j.optString("category");
            c.room = j.optString("room");
            c.kind = j.optString("kind");
            out.add(c);
        }
        // Tri par insertion (quelques dizaines de cours). Ni lambda (android.jar n'a pas
        // LambdaMetafactory pour javac), ni Comparator/Comparable : d8 34 plante sur leurs
        // méthodes « bridge » génériques compilées par javac 21.
        for (int i = 1; i < out.size(); i++) {
            Course c = out.get(i);
            int j = i - 1;
            while (j >= 0 && out.get(j).start > c.start) {
                out.set(j + 1, out.get(j));
                j--;
            }
            out.set(j + 1, c);
        }
        return out;
    }

    private static int kindColor(String k) {
        switch (k) {
            case "cm": return 0xFFFF5500;
            case "td": return 0xFF2E5BFF;
            case "tp": return 0xFFA855F7;
            case "exam": return 0xFFD4F000;
            default: return 0xFF22C55E;
        }
    }

    /** « 14:15 », « demain 09:45 », « lun. 09:45 ». */
    private static String when(long ms, long now) {
        long days = (midnight(ms) - midnight(now)) / 86_400_000L;
        if (days <= 0) return fmt("HH:mm", ms);
        if (days == 1) return "demain " + fmt("HH:mm", ms);
        return fmt("EEE HH:mm", ms);
    }

    private static String until(long ms, long now) {
        long min = (ms - now + 59_999) / 60_000;
        if (min < 60) return "dans " + min + " min";
        if (midnight(ms) == midnight(now)) return "dans " + (min / 60) + " h " + String.format(Locale.ROOT, "%02d", min % 60);
        return when(ms, now);
    }

    private static String line(Course c, long now) {
        return when(c.start, now) + "  " + c.title + (c.room.isEmpty() ? "" : "  · " + c.room);
    }

    static void renderEdt(Context ctx) {
        AppWidgetManager m = AppWidgetManager.getInstance(ctx);
        int[] ids = m.getAppWidgetIds(new ComponentName(ctx, EdtWidget.class));
        if (ids.length == 0) return;
        long now = System.currentTimeMillis();
        List<Course> all = courses(ctx);
        Course cur = null;
        List<Course> next = new ArrayList<>();
        for (Course c : all) {
            if (c.start <= now && now < c.end) {
                if (cur == null) cur = c;
            } else if (c.start > now) {
                next.add(c);
            }
        }
        RemoteViews v = new RemoteViews(ctx.getPackageName(), R.layout.widget_edt);
        Course main = cur != null ? cur : (next.isEmpty() ? null : next.remove(0));
        long wake = now + 10 * 60_000L;
        if (main == null) {
            v.setTextViewText(R.id.label, "EMPLOI DU TEMPS");
            v.setTextViewText(R.id.title, "Aucun cours à venir");
            v.setTextViewText(R.id.room, "Ouvre Cydera pour actualiser");
            v.setInt(R.id.stripe, "setBackgroundColor", MUTED);
            v.setViewVisibility(R.id.progress, View.GONE);
        } else {
            String cat = main.category.isEmpty() ? "" : main.category + " · ";
            if (cur != null) {
                v.setTextViewText(R.id.label, "EN COURS · fin à " + fmt("HH:mm", main.end));
                v.setTextColor(R.id.label, 0xFFFF3B30);
                int pct = (int) Math.max(0, Math.min(100, (now - main.start) * 100 / Math.max(1, main.end - main.start)));
                v.setProgressBar(R.id.progress, 100, pct, false);
                v.setViewVisibility(R.id.progress, View.VISIBLE);
                wake = Math.min(wake, main.end);
            } else {
                v.setTextViewText(R.id.label, "PROCHAIN COURS · " + until(main.start, now));
                v.setTextColor(R.id.label, ACCENT);
                v.setViewVisibility(R.id.progress, View.GONE);
                wake = Math.min(wake, main.start);
            }
            v.setTextViewText(R.id.title, main.title);
            String horaire = fmt("HH:mm", main.start) + "–" + fmt("HH:mm", main.end);
            if (cur == null && midnight(main.start) != midnight(now)) horaire = when(main.start, now) + "–" + fmt("HH:mm", main.end);
            v.setTextViewText(R.id.room, cat + horaire + (main.room.isEmpty() ? "" : " · " + main.room));
            v.setInt(R.id.stripe, "setBackgroundColor", kindColor(main.kind));
        }
        int[] rows = {R.id.next1, R.id.next2, R.id.next3, R.id.next4};
        for (int i = 0; i < rows.length; i++) {
            if (i < next.size()) {
                v.setTextViewText(rows[i], line(next.get(i), now));
                v.setViewVisibility(rows[i], View.VISIBLE);
            } else {
                v.setViewVisibility(rows[i], View.GONE);
            }
        }
        if (!next.isEmpty()) wake = Math.min(wake, next.get(0).start);
        v.setOnClickPendingIntent(R.id.root, openApp(ctx, 4));
        m.updateAppWidget(ids, v);
        // Prochain changement (fin du cours, début du suivant) ou dans 10 min au plus tard.
        // RTC (sans réveil) : rien ne se passe écran éteint, la mise à jour arrive au réveil.
        AlarmManager am = (AlarmManager) ctx.getSystemService(Context.ALARM_SERVICE);
        if (am != null) am.setWindow(AlarmManager.RTC, Math.max(wake, now + 30_000L), 60_000L, action(ctx, ACTION_REFRESH, 5));
    }

    static void renderAll(Context ctx) {
        renderChrono(ctx);
        renderEdt(ctx);
    }
}
