package fr.cydera.app;

import android.content.BroadcastReceiver;
import android.content.Context;
import android.content.Intent;

/**
 * Récepteur non exporté (aucune autre appli ne peut l'appeler) : appui sur ▶/⏸, rafraîchissement
 * demandé par l'app (JNI) ou réveil programmé du widget « Emploi du temps ».
 */
public class WidgetActions extends BroadcastReceiver {
    @Override
    public void onReceive(Context ctx, Intent intent) {
        String a = intent.getAction();
        if (WidgetData.ACTION_TOGGLE.equals(a)) {
            WidgetData.Chrono c = WidgetData.chrono(ctx);
            if (!c.subject.isEmpty()) WidgetData.command(ctx, c.running ? "pause" : "start");
            WidgetData.renderChrono(ctx);
        } else {
            WidgetData.renderAll(ctx);
        }
    }
}
