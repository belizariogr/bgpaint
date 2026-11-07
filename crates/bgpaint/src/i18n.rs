//! User interface strings. English and Brazilian Portuguese, chosen from the system locale.

macro_rules! strings {
    ($($key:ident: $en:expr, $pt:expr;)*) => {
        /// Every translatable string of the interface.
        pub struct Strings {
            $(pub $key: &'static str,)*
        }

        pub static EN: Strings = Strings { $($key: $en,)* };
        pub static PT_BR: Strings = Strings { $($key: $pt,)* };
    };
}

strings! {
    app_name: "Paint", "Paint";
    untitled: "Untitled", "Sem título";
    // Menus
    menu_file: "File", "Arquivo";
    menu_edit: "Edit", "Editar";
    menu_view: "View", "Exibir";
    new: "New", "Novo";
    open: "Open", "Abrir";
    import_to_canvas: "Import to canvas", "Importar para tela";
    from_file: "From a file", "De um arquivo";
    from_scanner: "From scanner or camera", "Do scanner ou câmera";
    recent: "Recent", "Recente";
    no_recent: "No recent files", "Nenhum arquivo recente";
    save: "Save", "Salvar";
    save_as: "Save as", "Salvar como";
    save_as_project: "Save as project", "Salvar como projeto";
    png_image: "PNG image", "Imagem PNG";
    jpeg_image: "JPEG image", "Imagem JPEG";
    bmp_image: "BMP image", "Imagem BMP";
    gif_image: "GIF image", "Imagem GIF";
    webp_image: "WEBP image", "Imagem WEBP";
    other_format: "Other format", "Outro formato";
    print: "Print", "Imprimir";
    page_setup: "Page setup", "Configurar página";
    print_preview: "Print preview", "Visualização de Impressão";
    share: "Share", "Compartilhar";
    set_as_wallpaper: "Set as desktop background", "Definir como tela de fundo da área de trabalho";
    wallpaper_fill: "Fill", "Preencher";
    wallpaper_tile: "Tile", "Lado a lado";
    wallpaper_center: "Center", "Centralizar";
    image_properties: "Image properties", "Propriedades da imagem";
    exit: "Exit", "Sair";
    undo: "Undo", "Desfazer";
    redo: "Redo", "Refazer";
    cut: "Cut", "Recortar";
    copy: "Copy", "Copiar";
    paste: "Paste", "Colar";
    copy_visible_layers: "Copy visible layers", "Copiar camadas visíveis";
    zoom: "Zoom", "Ampliar";
    zoom_100: "100%", "100%";
    zoom_in: "Zoom in", "Aplicar zoom";
    zoom_out: "Zoom out", "Reduzir";
    fit_to_window: "Fit to window", "Ajustar à janela";
    rulers: "Rulers", "Réguas";
    gridlines: "Gridlines", "Linhas de grade";
    status_bar: "Status bar", "Barra de status";
    always_show_toolbar: "Always show toolbar", "Sempre mostrar a barra de ferramentas";
    auto_hide_toolbar: "Automatically hide toolbar", "Ocultar a barra de ferramentas automaticamente";
    full_screen: "Full screen", "Tela inteira";
    thumbnail: "Thumbnail", "Miniatura";
    settings: "Settings", "Configurações";
    // Ribbon groups
    group_selection: "Selection", "Seleção";
    group_image: "Image", "Imagem";
    group_tools: "Tools", "Ferramentas";
    group_brushes: "Brushes", "Pincéis";
    group_shapes: "Shapes", "Formas";
    group_colors: "Colors", "Cores";
    group_layers: "Layers", "Camadas";
    // Selection
    rectangle_selection: "Rectangle", "Retângulo";
    freeform_selection: "Free-form", "Forma livre";
    select_all: "Select all", "Selecionar tudo";
    invert_selection: "Invert selection", "Inverter seleção";
    transparent_selection: "Transparent selection", "Seleção transparente";
    delete: "Delete", "Excluir";
    // Image
    crop: "Crop", "Cortar";
    resize_and_skew: "Resize and skew", "Alongar e distorcer";
    rotate: "Rotate", "Girar";
    rotate_right: "Rotate right 90°", "Girar 90° para a direita";
    rotate_left: "Rotate left 90°", "Girar 90° para a esquerda";
    rotate_180: "Rotate 180°", "Girar 180°";
    rotate_custom: "Custom rotate", "Girar personalizado";
    flip: "Flip", "Inverter";
    flip_vertical: "Flip vertical", "Inverter verticalmente";
    flip_horizontal: "Flip horizontal", "Inverter horizontalmente";
    invert_color: "Invert color", "Inverter cor";
    // Tools
    pencil: "Pencil", "Lápis";
    fill: "Fill with color", "Preencher com cor";
    text: "Text", "Texto";
    eraser: "Eraser", "Borracha";
    color_picker: "Color picker", "Seletor de cores";
    magnifier: "Magnifier", "Lupa";
    // Brushes
    brush: "Brush", "Pincel";
    calligraphy_brush: "Calligraphy brush", "Pincel de caligrafia";
    calligraphy_pen: "Calligraphy pen", "Caneta de caligrafia";
    airbrush: "Airbrush", "Spray";
    oil_brush: "Oil brush", "Pincel para óleo";
    crayon: "Crayon", "Giz de Cera";
    marker: "Marker", "Marcador";
    natural_pencil: "Natural pencil", "Lápis natural";
    watercolor: "Watercolor brush", "Pincel para aquarela";
    size: "Size", "Tamanho";
    // Shapes
    shape_line: "Line", "Linha";
    shape_curve: "Curve", "Curva";
    shape_oval: "Oval", "Oval";
    shape_rectangle: "Rectangle", "Retângulo";
    shape_rounded_rectangle: "Rounded rectangle", "Retângulo arredondado";
    shape_polygon: "Polygon", "Polígono";
    shape_triangle: "Triangle", "Triângulo";
    shape_right_triangle: "Right triangle", "Triângulo retângulo";
    shape_diamond: "Diamond", "Losango";
    shape_pentagon: "Pentagon", "Pentágono";
    shape_hexagon: "Hexagon", "Hexágono";
    shape_right_arrow: "Right arrow", "Seta para a direita";
    shape_left_arrow: "Left arrow", "Seta para a esquerda";
    shape_up_arrow: "Up arrow", "Seta para cima";
    shape_down_arrow: "Down arrow", "Seta para baixo";
    shape_star4: "Four-point star", "Estrela de quatro pontas";
    shape_star5: "Five-point star", "Estrela de cinco pontas";
    shape_star6: "Six-point star", "Estrela de seis pontas";
    shape_rounded_callout: "Rounded rectangular callout", "Texto explicativo retangular arredondado";
    shape_oval_callout: "Oval callout", "Texto explicativo oval";
    shape_cloud_callout: "Cloud callout", "Texto explicativo em nuvem";
    shape_heart: "Heart", "Coração";
    shape_lightning: "Lightning", "Raio";
    outline: "Outline", "Contorno";
    shape_fill: "Fill", "Preencher";
    no_outline: "No outline", "Sem contorno";
    solid_outline: "Solid color", "Contorno sólido";
    no_fill: "No fill", "Sem preenchimento";
    solid_fill: "Solid color", "Preenchimento sólido";
    // Colors
    color1: "Color 1", "Cor 1";
    color2: "Color 2", "Cor 2";
    edit_colors: "Edit colors", "Editar cores";
    basic_colors: "Basic colors", "Cores básicas";
    custom_colors: "Custom colors", "Cores personalizadas";
    red: "Red", "Vermelho";
    green: "Green", "Verde";
    blue: "Blue", "Azul";
    hue: "Hue", "Matiz";
    saturation: "Saturation", "Saturação";
    value: "Value", "Valor";
    // Layers
    layers: "Layers", "Camadas";
    add_layer: "Add layer", "Adicionar camada";
    hide_layer: "Hide layer", "Ocultar camada";
    show_layer: "Show layer", "Mostrar camada";
    duplicate_layer: "Duplicate layer", "Duplicar camada";
    merge_down: "Merge down", "Mesclar para baixo";
    move_up: "Move up", "Mover para cima";
    move_down: "Move down", "Mover para baixo";
    delete_layer: "Delete layer", "Excluir camada";
    background: "Background", "Tela de Fundo";
    hide_background: "Hide background", "Ocultar plano de fundo";
    show_background: "Show background", "Mostrar plano de fundo";
    change_color: "Change color", "Alterar a cor";
    layer_n: "Layer", "Camada";
    // Text
    background_fill: "Background fill", "Preenchimento de fundo";
    bold: "Bold", "Negrito";
    italic: "Italic", "Itálico";
    underline: "Underline", "Sublinhado";
    strikethrough: "Strikethrough", "Tachado";
    align_left: "Align left", "Alinhar à esquerda";
    align_center: "Center", "Centralizar";
    align_right: "Align right", "Alinhar à direita";
    font_family: "Font family", "Família da fonte";
    font_size: "Font size", "Tamanho da fonte";
    // Dialogs
    ok: "OK", "OK";
    cancel: "Cancel", "Cancelar";
    resize: "Resize", "Redimensionar";
    percentage: "Percentage", "Porcentagem";
    pixels: "Pixels", "Pixels";
    horizontal: "Horizontal", "Horizontal";
    vertical: "Vertical", "Vertical";
    skew: "Skew", "Inclinar";
    maintain_aspect_ratio: "Maintain aspect ratio", "Manter taxa de proporção";
    image_properties_title: "Image Properties", "Propriedades da Imagem";
    file_attributes: "File attributes", "Atributos de Arquivo";
    saved_at: "Saved at:", "Salvo em:";
    size_on_disk: "Size on disk:", "Tamanho em disco:";
    resolution: "Resolution:", "Resolução:";
    not_available: "Not available", "Não Disponível";
    units: "Units", "Unidades";
    inches: "Inches", "Polegadas";
    centimeters: "Centimeters", "Centímetros";
    image_size: "Image size", "Tamanho da imagem";
    width: "Width:", "Largura:";
    height: "Height:", "Altura:";
    default: "Default", "Padrão";
    angle: "Angle", "Ângulo";
    unsaved_title: "Paint", "Paint";
    unsaved_question: "Do you want to save your changes to", "Deseja salvar as alterações em";
    dont_save: "Don't save", "Não salvar";
    error_title: "Something went wrong", "Algo deu errado";
    open_failed: "This file could not be opened.", "Não foi possível abrir este arquivo.";
    save_failed: "The image could not be saved.", "Não foi possível salvar a imagem.";
    close: "Close", "Fechar";
    all_images: "All images", "Todas as imagens";
    paint_project: "Paint project", "Projeto do Paint";
    // Print
    printer: "Printer", "Impressora";
    copies: "Copies", "Cópias";
    orientation: "Orientation", "Orientação";
    portrait: "Portrait", "Retrato";
    landscape: "Landscape", "Paisagem";
    scaling: "Scaling", "Dimensionamento";
    fit_to_page: "Fit to page", "Ajustar à página";
    actual_size: "Actual size", "Tamanho real";
    margins: "Margins (mm)", "Margens (mm)";
    center_on_page: "Center on page", "Centralizar na página";
    paper_size: "Paper size", "Tamanho do papel";
    no_printers: "No printers found", "Nenhuma impressora encontrada";
    print_sent: "Sent to the printer", "Enviado para a impressora";
    print_failed: "Printing failed", "Falha na impressão";
    wallpaper_failed: "Could not set the desktop background", "Não foi possível definir a tela de fundo";
    wallpaper_done: "Desktop background updated", "Tela de fundo atualizada";
    share_failed: "Could not share the image", "Não foi possível compartilhar a imagem";
    // Settings page
    app_theme: "App theme", "Tema do aplicativo";
    theme_light: "Light", "Claro";
    theme_dark: "Dark", "Escuro";
    theme_system: "Use system setting", "Usar a configuração do sistema";
    about: "About Paint", "Sobre o Paint";
    about_text: "An open source Paint for Linux and macOS, written in Rust.", "Um Paint de código aberto para Linux e macOS, escrito em Rust.";
    source_code: "Source code", "Código-fonte";
    license: "License (MIT)", "Licença (MIT)";
    third_party: "Third-party notices", "Avisos de terceiros";
    // Status bar
    zoom_level: "Zoom level", "Nível de zoom";
    canvas_size: "Canvas size", "Tamanho da tela";
    cursor_position: "Cursor position", "Posição do cursor";
    selection_size: "Selection size", "Tamanho da seleção";
    expand_toolbar: "Show toolbar", "Mostrar barra de ferramentas";
    collapse_toolbar: "Hide toolbar", "Ocultar barra de ferramentas";
}

/// Chooses the strings for the current system locale.
pub fn detect() -> &'static Strings {
    let locale = sys_locale::get_locale().unwrap_or_default().to_ascii_lowercase();
    from_code(&locale)
}

pub fn from_code(code: &str) -> &'static Strings {
    if code.starts_with("pt") { &PT_BR } else { &EN }
}

#[cfg(test)]
mod tests {
    #[test]
    fn locale_selection() {
        assert_eq!(super::from_code("pt-br").new, "Novo");
        assert_eq!(super::from_code("pt_PT").new, "Novo");
        assert_eq!(super::from_code("en-US").new, "New");
        assert_eq!(super::from_code("").new, "New");
    }
}
