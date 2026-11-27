package fr.cydera.app;

import android.appwidget.AppWidgetManager;
import android.appwidget.AppWidgetProvider;
import android.content.Context;

/** Widget « Chrono » : matière, temps du jour qui défile, bouton ▶/⏸. */
public class ChronoWidget extends AppWidgetProvider {
    @Override
    public void onUpdate(Context ctx, AppWidgetManager m, int[] ids) {
        WidgetData.renderChrono(ctx);
    }
}
