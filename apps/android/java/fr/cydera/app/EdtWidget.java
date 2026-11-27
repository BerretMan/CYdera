package fr.cydera.app;

import android.appwidget.AppWidgetManager;
import android.appwidget.AppWidgetProvider;
import android.content.Context;

/** Widget « Emploi du temps » : cours actuel (ou prochain) et les suivants. */
public class EdtWidget extends AppWidgetProvider {
    @Override
    public void onUpdate(Context ctx, AppWidgetManager m, int[] ids) {
        WidgetData.renderEdt(ctx);
    }
}
