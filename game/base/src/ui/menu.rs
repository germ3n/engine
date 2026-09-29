use crate::state::GameState;
use crate::ui::window::Window;

pub fn draw_menu<T: Window, In, Out>(_window: &mut T, game: &mut GameState<In, Out>) {
    game.run_hook("MenuPaint", ()); // render the menu
}
