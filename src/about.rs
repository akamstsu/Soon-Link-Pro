use eframe::egui::*;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct Component {
    name: String,
    version: String,
    license: String,
    repository: String,
    notice: String,
}
fn components() -> &'static [Component] {
    static DATA: OnceLock<Vec<Component>> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/open-source.json"))
            .expect("bundled component manifest")
    })
}
#[derive(Default)]
pub struct About {
    pub open: bool,
    tab: usize,
    filter: String,
    selected: Option<usize>,
    logo: Option<TextureHandle>,
}
impl About {
    pub fn show(&mut self, ctx: &Context) {
        if !self.open {
            return;
        }
        if self.logo.is_none() {
            if let Ok(image) =
                image::load_from_memory(include_bytes!("../assets/zhibin-club-logo.png"))
            {
                let image = image
                    .resize(192, 192, image::imageops::FilterType::Triangle)
                    .to_rgba8();
                self.logo = Some(ctx.load_texture(
                    "zhibin_club_logo",
                    ColorImage::from_rgba_unmultiplied(
                        [image.width() as usize, image.height() as usize],
                        image.as_raw(),
                    ),
                    TextureOptions::LINEAR,
                ));
            }
        }
        let mut open = self.open;
        crate::ui_shell::dialog("关于与帮助 · Soon Link Pro").open(&mut open).default_width(690.0).collapsible(false).show(ctx, |ui| {
            crate::ui_shell::intro(ui, "Soon Link Pro", "R2.4.4-dev · Windows 音频路由工作台");
            ui.horizontal(|ui| {
                for (index, (icon, label)) in [(crate::widgets::Icon::Info, "关于"), (crate::widgets::Icon::Help, "使用指南"), (crate::widgets::Icon::More, "开源组件与版权")].iter().enumerate() {
                    if crate::widgets::action(ui, *icon, label, self.tab == index).clicked() { self.tab = index; }
                }
            });
            ui.add_space(12.0);
            if self.tab == 0 {
                crate::ui_shell::section().show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 18.0;
                        if let Some(logo) = &self.logo {
                            ui.add(Image::new((logo.id(), vec2(88.0, 88.0))).maintain_aspect_ratio(true));
                        }
                        ui.vertical(|ui| {
                            ui.add_space(6.0);
                            ui.label(RichText::new("© 2026 挚彬Club").size(20.0).strong());
                            ui.add_space(3.0);
                            ui.label("作者：赤松_Akamatsu");
                            ui.hyperlink_to("https://www.zhibin.club", "https://www.zhibin.club");
                        });
                    });
                    ui.add_space(8.0);
                    ui.label("软件原创部分版权归挚彬Club所有。");
                    ui.label("引用组件保留各自作者的版权、许可证及来源；上述署名不替代第三方声明。");
                    ui.add_space(10.0);
                    ui.label("音频设备：Windows WASAPI / ASIO；虚拟 Windows 端点由 usbip-audio 与 usbip-win2 提供。");
                    ui.label("本机微软雅黑由 Windows 加载，未随安装包分发。内置字体声明收录于 epaint_default_fonts 条目。");
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if crate::widgets::action(ui, crate::widgets::Icon::Help, "查看使用指南", false).clicked() { self.tab = 1; }
                    if crate::widgets::text_button(ui, "查看开源组件与完整许可", false).clicked() { self.tab = 2; }
                });
            } else if self.tab == 1 {
                ScrollArea::vertical().id_salt("help_content").max_height((ctx.screen_rect().height()-300.0).clamp(160.0, 460.0)).show(ui, |ui| {
                    for (title, body) in [
                        ("01  按需要添加", "默认提供一组虚拟输入／输出，并显示全部硬件通道。添加应用时，Windows 会话中正在发声的程序优先，应用名称优先使用程序自带中文说明，可搜索名称或路径；检测在后台进行。通过通道管理，按实际设备名称显示需要的硬件输入和输出。隐藏不会删除接线，也不会静音。"),
                        ("02  接线与取消选中", "同一设备的 L/R 放在一个通道组内，每行保留独立端口。点选或拖动左侧输入端口，再连接右侧目标端口，可交叉、混音或一对多。点击空白处或按 Esc 取消待接线选中；已有接线保留。点击连线中心可调整或移除连接。"),
                        ("03  音量与整组静音", "电平条也是音量滑块，白色竖标表示音量，彩色长度表示信号；拖动调整，双击恢复 0 dB。L/R 默认联动，点通道组标题栏的小链条解除后可分别拖动。旁边的扬声器图标一次静音或恢复整个通道组，包括隐藏声道，保留各声道音量；静音独立于联动开关。"),
                        ("04  通道组备注", "点击通道组标题或「··· → 设置通道组备注」，显示为原名 (备注)。所有通道组备注只保存在本软件，输入、输出各自独立；不修改 Windows 设备名、DAW ASIO 通道名、硬件或应用名称，也不重启音频。原始通道名称可在详情查看。"),
                        ("05  Windows 和 DAW 共用总线", "同一组 Windows 播放与 DAW 虚拟输出合流到左侧输入通道组；右侧输出通道组同时发送给 Windows 录音和 DAW 虚拟输入。ASIO 前段直接转发实体声卡输入／输出，后段追加虚拟通道。不要把 DAW 正在监听的输入再接回同一返回总线，以免反馈。"),
                        ("06  运行与设备管理", "软件默认自动启动。增删通道或改动音频配置会后台重开音频并请求 DAW 重载，期间声音会短暂中断。手动暂停后，编辑不会自行恢复音频。通道组菜单可移出面板，虚拟总线与应用可删除；接线、音量及静音实时生效，无需重开声卡。驱动安装和更新检查在「设置 → 虚拟设备与 DAW」。"),
                        ("07  动效与性能", "有实际信号时，光束向目标流动。「设置 → 界面与性能」可关闭光束或降低动画帧率，音频继续运行。最小化时停止画布绘制，端点检查和音频重开在后台处理。"),
                        ("08  默认通道与隐藏", "至少保留一组立体声虚拟输入／输出，以兼容要求非零通道的 DAW。最后一组可以隐藏；其接口仍然保留。新配置默认显示所有硬件输入和输出，之后可以在通道管理逐项隐藏；更新保留已经保存的隐藏状态。"),
                        ("09  DAW 与托盘", "DAW 初始化 ASIO 时可自动启动软件，最长等待音频就绪 20 秒。关闭窗口收进托盘，音频继续；单击图标显示，右键可退出。由 DAW 启动的实例在最后一个已登记宿主进程退出后结束；暂停或 ASIO 重载不会误触退出。手动打开的实例独立运行，也可从托盘选择保持独立运行。"),
                        ("10  安装与更新", "安装器自动注册 ASIO，并在未检测到 USB/IP 内核驱动时安装原厂驱动；已存在的驱动不重复安装、不降级。更新前保存并退出 DAW，从托盘退出软件。配置保存在用户数据目录，更新和卸载均保留；可选导入旧便携版配置，已有配置优先。虚拟端点按实际总线配置在音频运行时连接，不预建固定组数。"),
                    ] {
                        crate::ui_shell::section().show(ui, |ui| {
                            ui.label(RichText::new(title).size(15.0).color(Color32::from_rgb(180, 158, 255)).strong());
                            ui.add_space(5.0);
                            ui.add(Label::new(body).wrap());
                        });
                        ui.add_space(6.0);
                    }
                });
            } else {
                let entries = components();
                ui.label(format!("{} 项运行、构建依赖及随包组件；点击查看完整版权和许可证。", entries.len()));
                crate::widgets::search(ui, &mut self.filter, "搜索组件或许可证", f32::INFINITY);
                let needle = self.filter.to_lowercase();
                ScrollArea::vertical().id_salt("third_party_list").max_height((ctx.screen_rect().height() - 340.0).clamp(160.0, 410.0)).show(ui, |ui| {
                    for (index, component) in entries.iter().enumerate().filter(|(_, c)| needle.is_empty() || format!("{} {}", c.name, c.license).to_lowercase().contains(&needle)) {
                        ui.horizontal_wrapped(|ui| {
                            if crate::widgets::text_button(ui, format!("{}  {}", component.name, component.version), self.selected == Some(index)).clicked() { self.selected = Some(index); }
                            ui.label(RichText::new(&component.license).size(11.0).color(Color32::from_rgb(150, 162, 185)));
                        });
                    }
                });
                ui.label(RichText::new("完整清单、上游来源及许可文本也随包保存在 licenses / THIRD_PARTY_NOTICES.md。").size(11.0));
            }
        });
        self.open = open;
        if let Some(index) = self.selected {
            let component = &components()[index];
            let mut license_open = true;
            crate::ui_shell::dialog(format!("{} · 版权与许可证", component.name))
                .id(Id::new("component_license"))
                .open(&mut license_open)
                .default_size([710.0, 520.0])
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(format!("{} {}", component.name, component.version))
                            .size(18.0)
                            .strong(),
                    );
                    ui.label(&component.license);
                    if !component.repository.is_empty() {
                        ui.hyperlink_to("上游项目与源代码", &component.repository);
                    }
                    ScrollArea::vertical()
                        .id_salt(("license_text", index))
                        .max_height((ctx.screen_rect().height() - 220.0).clamp(180.0, 510.0))
                        .show(ui, |ui| {
                            ui.add(
                                Label::new(RichText::new(&component.notice).size(12.0))
                                    .wrap()
                                    .selectable(true),
                            );
                        });
                });
            if !license_open {
                self.selected = None;
            }
        }
    }
}
