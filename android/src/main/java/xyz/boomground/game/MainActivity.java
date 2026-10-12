package xyz.boomground.game;

import android.os.Bundle;
import android.view.View;
import android.view.WindowManager;

/**
 * Activité Android de BoomGround.
 *
 * Elle ne fait presque rien : {@code NativeActivity} charge
 * {@code libboomground.so} et lui transmet l'application, et c'est
 * {@code android_main} (généré par {@code #[bevy_main]}) qui prend la main.
 *
 * On ajoute seulement :
 *
 * - le mode immersif, pour que le jeu occupe tout l'écran (le jeu se joue
 *   en portrait, cf. {@code android:screenOrientation} dans le manifeste) ;
 * - le maintien de l'écran allumé pendant une partie.
 */
public class MainActivity extends android.app.NativeActivity {

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        goImmersive();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        // Les barres système reviennent au glissement ; on les masque
        // dès que le jeu reprend la main.
        if (hasFocus) {
            goImmersive();
        }
    }

    /** Masque les barres système et affiche le jeu en plein écran. */
    private void goImmersive() {
        View decor = getWindow().getDecorView();
        decor.setSystemUiVisibility(
                View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                        | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                        | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                        | View.SYSTEM_UI_FLAG_FULLSCREEN
                        | View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY);
    }
}
