use crate::input::{
    command::RunCommand,
    config::ConfigError,
    layout::{
        FloatingPaneLayout, Layout, TiledPaneLayout, PercentOrFixed, Run, RunPlugin, RunPluginLocation,
        SplitDirection, SplitSize, SwapTiledLayout, SwapFloatingLayout, LayoutConstraint
    },
};

use kdl::*;

use std::collections::{HashMap, HashSet, BTreeMap};
use std::str::FromStr;

use crate::{
    kdl_child_with_name, kdl_children_nodes, kdl_get_bool_property_or_child_value,
    kdl_get_bool_property_or_child_value_with_error, kdl_get_child,
    kdl_get_int_property_or_child_value, kdl_get_property_or_child,
    kdl_get_string_property_or_child_value, kdl_get_string_property_or_child_value_with_error,
    kdl_name, kdl_parsing_error, kdl_property_names, kdl_property_or_child_value_node,
    kdl_string_arguments,
};

use std::convert::TryFrom;
use std::path::PathBuf;
use std::vec::Vec;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneOrFloatingPane {
    Pane(TiledPaneLayout),
    FloatingPane(FloatingPaneLayout),
    Either(TiledPaneLayout),
}

pub struct KdlLayoutParser<'a> {
    global_cwd: Option<PathBuf>,
    raw_layout: &'a str,
    tab_templates: HashMap<String, (TiledPaneLayout, Vec<FloatingPaneLayout>, KdlNode)>,
    pane_templates: HashMap<String, (PaneOrFloatingPane, KdlNode)>,
    default_tab_template: Option<(TiledPaneLayout, Vec<FloatingPaneLayout>, KdlNode)>,
}

impl<'a> KdlLayoutParser<'a> {
    pub fn new(raw_layout: &'a str, global_cwd: Option<PathBuf>) -> Self {
        KdlLayoutParser {
            raw_layout,
            tab_templates: HashMap::new(),
            pane_templates: HashMap::new(),
            default_tab_template: None,
            global_cwd,
        }
    }
    fn is_a_reserved_word(&self, word: &str) -> bool {
        word == "pane"
            || word == "layout"
            || word == "pane_template"
            || word == "tab_template"
            || word == "default_tab_template"
            || word == "command"
            || word == "edit"
            || word == "plugin"
            || word == "children"
            || word == "tab"
            || word == "args"
            || word == "close_on_exit"
            || word == "start_suspended"
            || word == "borderless"
            || word == "focus"
            || word == "name"
            || word == "size"
            || word == "cwd"
            || word == "split_direction"
            || word == "swap_tiled_layout"
            || word == "swap_floating_layout"
    }
    fn is_a_valid_pane_property(&self, property_name: &str) -> bool {
        property_name == "borderless"
            || property_name == "focus"
            || property_name == "name"
            || property_name == "size"
            || property_name == "plugin"
            || property_name == "command"
            || property_name == "edit"
            || property_name == "cwd"
            || property_name == "args"
            || property_name == "close_on_exit"
            || property_name == "start_suspended"
            || property_name == "split_direction"
            || property_name == "pane"
            || property_name == "children"
    }
    fn is_a_valid_floating_pane_property(&self, property_name: &str) -> bool {
        property_name == "borderless"
            || property_name == "focus"
            || property_name == "name"
            || property_name == "plugin"
            || property_name == "command"
            || property_name == "edit"
            || property_name == "cwd"
            || property_name == "args"
            || property_name == "close_on_exit"
            || property_name == "start_suspended"
            || property_name == "x"
            || property_name == "y"
            || property_name == "width"
            || property_name == "height"
    }
    fn is_a_valid_tab_property(&self, property_name: &str) -> bool {
        property_name == "focus"
            || property_name == "name"
            || property_name == "split_direction"
            || property_name == "cwd"
            || property_name == "floating_panes"
            || property_name == "children"
            || property_name == "max_panes"
            || property_name == "min_panes"
    }
    fn assert_legal_node_name(&self, name: &str, kdl_node: &KdlNode) -> Result<(), ConfigError> {
        if name.contains(char::is_whitespace) {
            Err(ConfigError::new_layout_kdl_error(
                format!("Node names ({}) cannot contain whitespace.", name),
                kdl_node.span().offset(),
                kdl_node.span().len(),
            ))
        } else if self.is_a_reserved_word(&name) {
            Err(ConfigError::new_layout_kdl_error(
                format!("Node name '{}' is a reserved word.", name),
                kdl_node.span().offset(),
                kdl_node.span().len(),
            ))
        } else {
            Ok(())
        }
    }
    fn assert_legal_template_name(
        &self,
        name: &str,
        kdl_node: &KdlNode,
    ) -> Result<(), ConfigError> {
        if name.is_empty() {
            Err(ConfigError::new_layout_kdl_error(
                format!("Template names cannot be empty"),
                kdl_node.span().offset(),
                kdl_node.span().len(),
            ))
        } else if name.contains(')') || name.contains('(') {
            Err(ConfigError::new_layout_kdl_error(
                format!("Template names cannot contain parantheses"),
                kdl_node.span().offset(),
                kdl_node.span().len(),
            ))
        } else if name
            .chars()
            .next()
            .map(|first_char| first_char.is_numeric())
            .unwrap_or(false)
        {
            Err(ConfigError::new_layout_kdl_error(
                format!("Template names cannot start with numbers"),
                kdl_node.span().offset(),
                kdl_node.span().len(),
            ))
        } else {
            Ok(())
        }
    }
    fn parse_split_size(&self, kdl_node: &KdlNode) -> Result<Option<SplitSize>, ConfigError> {
        if let Some(size) = kdl_get_string_property_or_child_value!(kdl_node, "size") {
            match SplitSize::from_str(size) {
                Ok(size) => Ok(Some(size)),
                Err(_e) => Err(kdl_parsing_error!(
                    format!(
                        "size should be a fixed number (eg. 1) or a quoted percent (eg. \"50%\")"
                    ),
                    kdl_node
                )),
            }
        } else if let Some(size) = kdl_get_int_property_or_child_value!(kdl_node, "size") {
            if size == 0 {
                return Err(kdl_parsing_error!(
                    format!("size should be greater than 0"),
                    kdl_node
                ));
            }
            Ok(Some(SplitSize::Fixed(size as usize)))
        } else if let Some(node) = kdl_property_or_child_value_node!(kdl_node, "size") {
            Err(kdl_parsing_error!(
                format!("size should be a fixed number (eg. 1) or a quoted percent (eg. \"50%\")"),
                node
            ))
        } else if let Some(node) = kdl_child_with_name!(kdl_node, "size") {
            Err(kdl_parsing_error!(
                format!(
                    "size cannot be bare, it should have a value (eg. 'size 1', or 'size \"50%\"')"
                ),
                node
            ))
        } else {
            Ok(None)
        }
    }
    fn parse_percent_or_fixed(
        &self,
        kdl_node: &KdlNode,
        value_name: &str,
        can_be_zero: bool,
    ) -> Result<Option<PercentOrFixed>, ConfigError> {
        if let Some(size) = kdl_get_string_property_or_child_value!(kdl_node, value_name) {
            match PercentOrFixed::from_str(size) {
                Ok(size) => {
                    if !can_be_zero && size.is_zero() {
                        Err(kdl_parsing_error!(
                            format!("{} should be greater than 0", value_name),
                            kdl_node
                        ))
                    } else {
                        Ok(Some(size))
                    }
                },
                Err(_e) => Err(kdl_parsing_error!(
                    format!(
                        "{} should be a fixed number (eg. 1) or a quoted percent (eg. \"50%\")",
                        value_name
                    ),
                    kdl_node
                )),
            }
        } else if let Some(size) = kdl_get_int_property_or_child_value!(kdl_node, value_name) {
            if size == 0 && !can_be_zero {
                return Err(kdl_parsing_error!(
                    format!("{} should be greater than 0", value_name),
                    kdl_node
                ));
            }
            Ok(Some(PercentOrFixed::Fixed(size as usize)))
        } else if let Some(node) = kdl_property_or_child_value_node!(kdl_node, "size") {
            Err(kdl_parsing_error!(
                format!(
                    "{} should be a fixed number (eg. 1) or a quoted percent (eg. \"50%\")",
                    value_name
                ),
                node
            ))
        } else if let Some(node) = kdl_child_with_name!(kdl_node, "size") {
            Err(kdl_parsing_error!(
                format!(
                    "{} cannot be bare, it should have a value (eg. 'size 1', or 'size \"50%\"')",
                    value_name
                ),
                node
            ))
        } else {
            Ok(None)
        }
    }
    fn parse_plugin_block(&self, plugin_block: &KdlNode) -> Result<Option<Run>, ConfigError> {
        let _allow_exec_host_cmd =
            kdl_get_bool_property_or_child_value_with_error!(plugin_block, "_allow_exec_host_cmd")
                .unwrap_or(false);
        let string_url =
            kdl_get_string_property_or_child_value_with_error!(plugin_block, "location").ok_or(
                ConfigError::new_layout_kdl_error(
                    "Plugins must have a location".into(),
                    plugin_block.span().offset(),
                    plugin_block.span().len(),
                ),
            )?;
        let url_node = kdl_get_property_or_child!(plugin_block, "location").ok_or(
            ConfigError::new_layout_kdl_error(
                "Plugins must have a location".into(),
                plugin_block.span().offset(),
                plugin_block.span().len(),
            ),
        )?;
        let url = Url::parse(string_url).map_err(|e| {
            ConfigError::new_layout_kdl_error(
                format!("Failed to parse url: {:?}", e),
                url_node.span().offset(),
                url_node.span().len(),
            )
        })?;
        let location = RunPluginLocation::try_from(url)?;
        Ok(Some(Run::Plugin(RunPlugin {
            _allow_exec_host_cmd,
            location,
        })))
    }
    fn parse_args(&self, pane_node: &KdlNode) -> Result<Option<Vec<String>>, ConfigError> {
        match kdl_get_child!(pane_node, "args") {
            Some(kdl_args) => {
                if kdl_args.entries().is_empty() {
                    return Err(kdl_parsing_error!(format!("args cannot be empty and should contain one or more command arguments (eg. args \"-h\" \"-v\")"), kdl_args));
                }
                Ok(Some(
                    kdl_string_arguments!(kdl_args)
                        .iter()
                        .map(|s| String::from(*s))
                        .collect(),
                ))
            },
            None => Ok(None),
        }
    }
    fn cwd_prefix(&self, tab_cwd: Option<&PathBuf>) -> Result<Option<PathBuf>, ConfigError> {
        Ok(match (&self.global_cwd, tab_cwd) {
            (Some(global_cwd), Some(tab_cwd)) => Some(global_cwd.join(tab_cwd)),
            (None, Some(tab_cwd)) => Some(tab_cwd.clone()),
            (Some(global_cwd), None) => Some(global_cwd.clone()),
            (None, None) => None,
        })
    }
    fn parse_cwd(&self, kdl_node: &KdlNode) -> Result<Option<PathBuf>, ConfigError> {
        Ok(
            kdl_get_string_property_or_child_value_with_error!(kdl_node, "cwd")
                .map(|cwd| PathBuf::from(cwd)),
        )
    }
    fn parse_pane_command(
        &self,
        pane_node: &KdlNode,
        is_template: bool,
    ) -> Result<Option<Run>, ConfigError> {
        let command = kdl_get_string_property_or_child_value_with_error!(pane_node, "command")
            .map(|c| PathBuf::from(c));
        let edit = kdl_get_string_property_or_child_value_with_error!(pane_node, "edit")
            .map(|c| PathBuf::from(c));
        let cwd = self.parse_cwd(pane_node)?;
        let args = self.parse_args(pane_node)?;
        let close_on_exit =
            kdl_get_bool_property_or_child_value_with_error!(pane_node, "close_on_exit");
        let start_suspended =
            kdl_get_bool_property_or_child_value_with_error!(pane_node, "start_suspended");
        if !is_template {
            self.assert_no_bare_attributes_in_pane_node(
                &command,
                &args,
                &close_on_exit,
                &start_suspended,
                pane_node,
            )?;
        }
        let hold_on_close = close_on_exit.map(|c| !c).unwrap_or(true);
        let hold_on_start = start_suspended.map(|c| c).unwrap_or(false);
        match (command, edit, cwd) {
            (None, None, Some(cwd)) => Ok(Some(Run::Cwd(cwd))),
            (Some(command), None, cwd) => Ok(Some(Run::Command(RunCommand {
                command,
                args: args.unwrap_or_else(|| vec![]),
                cwd,
                hold_on_close,
                hold_on_start,
            }))),
            (None, Some(edit), Some(cwd)) => Ok(Some(Run::EditFile(cwd.join(edit), None))),
            (None, Some(edit), None) => Ok(Some(Run::EditFile(edit, None))),
            (Some(_command), Some(_edit), _) => Err(ConfigError::new_layout_kdl_error(
                "cannot have both a command and an edit instruction for the same pane".into(),
                pane_node.span().offset(),
                pane_node.span().len(),
            )),
            _ => Ok(None),
        }
    }
    fn parse_command_plugin_or_edit_block(
        &self,
        kdl_node: &KdlNode,
    ) -> Result<Option<Run>, ConfigError> {
        let mut run = self.parse_pane_command(kdl_node, false)?;
        if let Some(plugin_block) = kdl_get_child!(kdl_node, "plugin") {
            let has_non_cwd_run_prop = run
                .map(|r| match r {
                    Run::Cwd(_) => false,
                    _ => true,
                })
                .unwrap_or(false);
            if has_non_cwd_run_prop {
                return Err(ConfigError::new_layout_kdl_error(
                    "Cannot have both a command/edit and a plugin block for a single pane".into(),
                    plugin_block.span().offset(),
                    plugin_block.span().len(),
                ));
            }
            run = self.parse_plugin_block(plugin_block)?;
        }
        Ok(run)
    }
    fn parse_command_plugin_or_edit_block_for_template(
        &self,
        kdl_node: &KdlNode,
    ) -> Result<Option<Run>, ConfigError> {
        let mut run = self.parse_pane_command(kdl_node, true)?;
        if let Some(plugin_block) = kdl_get_child!(kdl_node, "plugin") {
            let has_non_cwd_run_prop = run
                .map(|r| match r {
                    Run::Cwd(_) => false,
                    _ => true,
                })
                .unwrap_or(false);
            if has_non_cwd_run_prop {
                return Err(ConfigError::new_layout_kdl_error(
                    "Cannot have both a command/edit and a plugin block for a single pane".into(),
                    plugin_block.span().offset(),
                    plugin_block.span().len(),
                ));
            }
            run = self.parse_plugin_block(plugin_block)?;
        }
        Ok(run)
    }
    fn parse_pane_node(&self, kdl_node: &KdlNode) -> Result<TiledPaneLayout, ConfigError> {
        self.assert_valid_pane_properties(kdl_node)?;
        let borderless = kdl_get_bool_property_or_child_value_with_error!(kdl_node, "borderless");
        let focus = kdl_get_bool_property_or_child_value_with_error!(kdl_node, "focus");
        let name = kdl_get_string_property_or_child_value_with_error!(kdl_node, "name")
            .map(|name| name.to_string());
        let split_size = self.parse_split_size(kdl_node)?;
        let run = self.parse_command_plugin_or_edit_block(kdl_node)?;
        let children_split_direction = self.parse_split_direction(kdl_node)?;
        let (external_children_index, children_are_stacked, children) = match kdl_children_nodes!(kdl_node) {
            Some(children) => self.parse_child_pane_nodes_for_pane(&children)?,
            None => (None, false, vec![]),
        };
        self.assert_no_mixed_children_and_properties(kdl_node)?;
        Ok(TiledPaneLayout {
            borderless: borderless.unwrap_or_default(),
            focus,
            name,
            split_size,
            run,
            children_split_direction,
            external_children_index,
            children,
            children_are_stacked,
            ..Default::default()
        })
    }
    fn parse_floating_pane_node(
        &self,
        kdl_node: &KdlNode,
    ) -> Result<FloatingPaneLayout, ConfigError> {
        self.assert_valid_floating_pane_properties(kdl_node)?;
        let height = self.parse_percent_or_fixed(kdl_node, "height", false)?;
        let width = self.parse_percent_or_fixed(kdl_node, "width", false)?;
        let x = self.parse_percent_or_fixed(kdl_node, "x", true)?;
        let y = self.parse_percent_or_fixed(kdl_node, "y", true)?;
        let run = self.parse_command_plugin_or_edit_block(kdl_node)?;
        let focus = kdl_get_bool_property_or_child_value_with_error!(kdl_node, "focus");
        let name = kdl_get_string_property_or_child_value_with_error!(kdl_node, "name")
            .map(|name| name.to_string());
        self.assert_no_mixed_children_and_properties(kdl_node)?;
        Ok(FloatingPaneLayout {
            name,
            height,
            width,
            x,
            y,
            run,
            focus,
            ..Default::default()
        })
    }
    fn insert_children_to_pane_template(
        &self,
        kdl_node: &KdlNode,
        pane_template: &mut TiledPaneLayout,
        pane_template_kdl_node: &KdlNode,
    ) -> Result<(), ConfigError> {
        let children_split_direction = self.parse_split_direction(kdl_node)?;
        let (external_children_index, children_are_stacked, pane_parts) = match kdl_children_nodes!(kdl_node) {
            Some(children) => self.parse_child_pane_nodes_for_pane(&children)?,
            None => (None, false, vec![]),
        };
        if pane_parts.len() > 0 {
            let child_panes_layout = TiledPaneLayout {
                children_split_direction,
                children: pane_parts,
                external_children_index,
                children_are_stacked,
                ..Default::default()
            };
            self.assert_one_children_block(&pane_template, pane_template_kdl_node)?;
            self.insert_layout_children_or_error(
                pane_template,
                child_panes_layout,
                pane_template_kdl_node,
            )?;
        }
        Ok(())
    }
    fn populate_external_children_index(&self, kdl_node: &KdlNode) -> Result<Option<(usize, bool)>, ConfigError> { // Option<(external_children_index, is_stacked)>
        if let Some(pane_child_nodes) = kdl_children_nodes!(kdl_node) {
            for (i, child) in pane_child_nodes.iter().enumerate() {
                if kdl_name!(child) == "children" {
                    let stacked =
                        kdl_get_bool_property_or_child_value_with_error!(kdl_node, "stacked").unwrap_or(false);



                    // TODO: BRING ME BACK!! need to adjust this to ignore "stacked"
//                     let node_has_child_nodes = child.children().map(|c| !c.is_empty()).unwrap_or(false);
//                     let node_has_entries = !child.entries().is_empty();
//                     if node_has_child_nodes || node_has_entries {
//                         return Err(ConfigError::new_layout_kdl_error(
//                             format!("The `children` node must be bare. All properties should be placed on the node consuming this template."),
//                             child.span().offset(),
//                             child.span().len(),
//                         ));
//                     }
                    return Ok(Some((i, stacked)));
                }
            }
        }
        return Ok(None);
    }
    fn parse_pane_node_with_template(
        &self,
        kdl_node: &KdlNode,
        pane_template: PaneOrFloatingPane,
        should_mark_external_children_index: bool,
        pane_template_kdl_node: &KdlNode,
    ) -> Result<TiledPaneLayout, ConfigError> {
        match pane_template {
            PaneOrFloatingPane::Pane(mut pane_template)
            | PaneOrFloatingPane::Either(mut pane_template) => {
                let borderless =
                    kdl_get_bool_property_or_child_value_with_error!(kdl_node, "borderless");
                let focus = kdl_get_bool_property_or_child_value_with_error!(kdl_node, "focus");
                let name = kdl_get_string_property_or_child_value_with_error!(kdl_node, "name")
                    .map(|name| name.to_string());
                let args = self.parse_args(kdl_node)?;
                let close_on_exit =
                    kdl_get_bool_property_or_child_value_with_error!(kdl_node, "close_on_exit");
                let start_suspended =
                    kdl_get_bool_property_or_child_value_with_error!(kdl_node, "start_suspended");
                let split_size = self.parse_split_size(kdl_node)?;
                let run = self.parse_command_plugin_or_edit_block_for_template(kdl_node)?;

                // TODO: change should_insert_children to should_keep_pane_external_children_index
                // or smth
                let external_children_index_and_is_stacked = if should_mark_external_children_index {
                    self.populate_external_children_index(kdl_node)?
                } else {
                    None
                };
                self.assert_no_bare_attributes_in_pane_node_with_template(
                    &run,
                    &pane_template.run,
                    &args,
                    &close_on_exit,
                    &start_suspended,
                    kdl_node,
                )?;
                self.insert_children_to_pane_template(
                    kdl_node,
                    &mut pane_template,
                    pane_template_kdl_node,
                )?;
                pane_template.run = Run::merge(&pane_template.run, &run);
                if let Some(pane_template_run_command) = pane_template.run.as_mut() {
                    // we need to do this because panes consuming a pane_template
                    // can have bare args without a command
                    pane_template_run_command.add_args(args);
                    pane_template_run_command.add_close_on_exit(close_on_exit);
                    pane_template_run_command.add_start_suspended(start_suspended);
                };
                if let Some(borderless) = borderless {
                    pane_template.borderless = borderless;
                }
                if let Some(focus) = focus {
                    pane_template.focus = Some(focus);
                }
                if let Some(name) = name {
                    pane_template.name = Some(name);
                }
                if let Some(split_size) = split_size {
                    pane_template.split_size = Some(split_size);
                }
                if let Some(index_of_children) = pane_template.external_children_index {
                    pane_template
                        .children
                        .insert(index_of_children, TiledPaneLayout::default());
                }
                pane_template.external_children_index = external_children_index_and_is_stacked.map(|(index, _is_stacked)| index);
                pane_template.children_are_stacked = external_children_index_and_is_stacked.map(|(_index, is_stacked)| is_stacked).unwrap_or(false);
                Ok(pane_template)
            },
            PaneOrFloatingPane::FloatingPane(_) => {
                let pane_template_name = kdl_get_string_property_or_child_value_with_error!(
                    pane_template_kdl_node,
                    "name"
                )
                .map(|name| name.to_string());
                Err(ConfigError::new_layout_kdl_error(
                    format!("pane_template {}, is a floating pane template (derived from its properties) and cannot be applied to a tiled pane", pane_template_name.unwrap_or("".into())),
                    kdl_node.span().offset(),
                    kdl_node.span().len(),
                ))
            },
        }
    }
    fn parse_floating_pane_node_with_template(
        &self,
        kdl_node: &KdlNode,
        pane_template: PaneOrFloatingPane,
        pane_template_kdl_node: &KdlNode,
    ) -> Result<FloatingPaneLayout, ConfigError> {
        match pane_template {
            PaneOrFloatingPane::Pane(_) => {
                let pane_template_name = kdl_get_string_property_or_child_value_with_error!(
                    pane_template_kdl_node,
                    "name"
                )
                .map(|name| name.to_string());
                Err(ConfigError::new_layout_kdl_error(
                    format!("pane_template {}, is a non-floating pane template (derived from its properties) and cannot be applied to a floating pane", pane_template_name.unwrap_or("".into())),
                    kdl_node.span().offset(),
                    kdl_node.span().len(),
                ))
            },
            PaneOrFloatingPane::FloatingPane(mut pane_template) => {
                let focus = kdl_get_bool_property_or_child_value_with_error!(kdl_node, "focus");
                let name = kdl_get_string_property_or_child_value_with_error!(kdl_node, "name")
                    .map(|name| name.to_string());
                let args = self.parse_args(kdl_node)?;
                let close_on_exit =
                    kdl_get_bool_property_or_child_value_with_error!(kdl_node, "close_on_exit");
                let start_suspended =
                    kdl_get_bool_property_or_child_value_with_error!(kdl_node, "start_suspended");
                let run = self.parse_command_plugin_or_edit_block_for_template(kdl_node)?;
                self.assert_no_bare_attributes_in_pane_node_with_template(
                    &run,
                    &pane_template.run,
                    &args,
                    &close_on_exit,
                    &start_suspended,
                    kdl_node,
                )?;
                pane_template.run = Run::merge(&pane_template.run, &run);
                if let Some(pane_template_run_command) = pane_template.run.as_mut() {
                    // we need to do this because panes consuming a pane_template
                    // can have bare args without a command
                    pane_template_run_command.add_args(args);
                    pane_template_run_command.add_close_on_exit(close_on_exit);
                    pane_template_run_command.add_start_suspended(start_suspended);
                };
                if let Some(focus) = focus {
                    pane_template.focus = Some(focus);
                }
                if let Some(name) = name {
                    pane_template.name = Some(name);
                }
                let height = self.parse_percent_or_fixed(kdl_node, "height", false)?;
                let width = self.parse_percent_or_fixed(kdl_node, "width", false)?;
                let x = self.parse_percent_or_fixed(kdl_node, "x", true)?;
                let y = self.parse_percent_or_fixed(kdl_node, "y", true)?;
                // let mut floating_pane = FloatingPaneLayout::from(&pane_template);
                if let Some(height) = height {
                    pane_template.height = Some(height);
                }
                if let Some(width) = width {
                    pane_template.width = Some(width);
                }
                if let Some(y) = y {
                    pane_template.y = Some(y);
                }
                if let Some(x) = x {
                    pane_template.x = Some(x);
                }
                Ok(pane_template)
            },
            PaneOrFloatingPane::Either(mut pane_template) => {
                let focus = kdl_get_bool_property_or_child_value_with_error!(kdl_node, "focus");
                let name = kdl_get_string_property_or_child_value_with_error!(kdl_node, "name")
                    .map(|name| name.to_string());
                let args = self.parse_args(kdl_node)?;
                let close_on_exit =
                    kdl_get_bool_property_or_child_value_with_error!(kdl_node, "close_on_exit");
                let start_suspended =
                    kdl_get_bool_property_or_child_value_with_error!(kdl_node, "start_suspended");
                let run = self.parse_command_plugin_or_edit_block_for_template(kdl_node)?;
                self.assert_no_bare_attributes_in_pane_node_with_template(
                    &run,
                    &pane_template.run,
                    &args,
                    &close_on_exit,
                    &start_suspended,
                    kdl_node,
                )?;
                pane_template.run = Run::merge(&pane_template.run, &run);
                if let Some(pane_template_run_command) = pane_template.run.as_mut() {
                    // we need to do this because panes consuming a pane_template
                    // can have bare args without a command
                    pane_template_run_command.add_args(args);
                    pane_template_run_command.add_close_on_exit(close_on_exit);
                    pane_template_run_command.add_start_suspended(start_suspended);
                };
                if let Some(focus) = focus {
                    pane_template.focus = Some(focus);
                }
                if let Some(name) = name {
                    pane_template.name = Some(name);
                }
                let height = self.parse_percent_or_fixed(kdl_node, "height", false)?;
                let width = self.parse_percent_or_fixed(kdl_node, "width", false)?;
                let x = self.parse_percent_or_fixed(kdl_node, "x", true)?;
                let y = self.parse_percent_or_fixed(kdl_node, "y", true)?;
                let mut floating_pane = FloatingPaneLayout::from(&pane_template);
                if let Some(height) = height {
                    floating_pane.height = Some(height);
                }
                if let Some(width) = width {
                    floating_pane.width = Some(width);
                }
                if let Some(y) = y {
                    floating_pane.y = Some(y);
                }
                if let Some(x) = x {
                    floating_pane.x = Some(x);
                }
                Ok(floating_pane)
            },
        }
    }
    fn parse_split_direction(&self, kdl_node: &KdlNode) -> Result<SplitDirection, ConfigError> {
        match kdl_get_string_property_or_child_value_with_error!(kdl_node, "split_direction") {
            Some(direction) => match SplitDirection::from_str(direction) {
                Ok(split_direction) => Ok(split_direction),
                Err(_e) => Err(kdl_parsing_error!(
                    format!(
                        "split_direction should be either \"horizontal\" or \"vertical\" found: {}",
                        direction
                    ),
                    kdl_node
                )),
            },
            None => Ok(SplitDirection::default()),
        }
    }
    fn has_only_neutral_pane_template_properties(
        &self,
        kdl_node: &KdlNode,
    ) -> Result<bool, ConfigError> {
        // pane properties
        let borderless = kdl_get_bool_property_or_child_value_with_error!(kdl_node, "borderless");
        let split_size = self.parse_split_size(kdl_node)?;
        let split_direction =
            kdl_get_string_property_or_child_value_with_error!(kdl_node, "split_direction");
        let has_children_nodes = self.has_child_nodes(kdl_node);

        // floating pane properties
        let height = self.parse_percent_or_fixed(kdl_node, "height", false)?;
        let width = self.parse_percent_or_fixed(kdl_node, "width", false)?;
        let x = self.parse_percent_or_fixed(kdl_node, "x", true)?;
        let y = self.parse_percent_or_fixed(kdl_node, "y", true)?;

        let has_pane_properties = borderless.is_some()
            || split_size.is_some()
            || split_direction.is_some()
            || has_children_nodes;
        let has_floating_pane_properties =
            height.is_some() || width.is_some() || x.is_some() || y.is_some();
        if has_pane_properties || has_floating_pane_properties {
            Ok(false)
        } else {
            Ok(true)
        }
    }
    fn differentiate_pane_and_floating_pane_template(
        &self,
        kdl_node: &KdlNode,
    ) -> Result<bool, ConfigError> {
        // returns true if it's a floating_pane template, false if not

        // pane properties
        let borderless = kdl_get_bool_property_or_child_value_with_error!(kdl_node, "borderless");
        let split_size = self.parse_split_size(kdl_node)?;
        let split_direction =
            kdl_get_string_property_or_child_value_with_error!(kdl_node, "split_direction");
        let has_children_nodes = self.has_child_nodes(kdl_node);

        // floating pane properties
        let height = self.parse_percent_or_fixed(kdl_node, "height", false)?;
        let width = self.parse_percent_or_fixed(kdl_node, "width", false)?;
        let x = self.parse_percent_or_fixed(kdl_node, "x", true)?;
        let y = self.parse_percent_or_fixed(kdl_node, "y", true)?;

        let has_pane_properties = borderless.is_some()
            || split_size.is_some()
            || split_direction.is_some()
            || has_children_nodes;
        let has_floating_pane_properties =
            height.is_some() || width.is_some() || x.is_some() || y.is_some();

        if has_pane_properties && has_floating_pane_properties {
            let mut pane_properties = vec![];
            if borderless.is_some() {
                pane_properties.push("borderless");
            }
            if split_size.is_some() {
                pane_properties.push("split_size");
            }
            if split_direction.is_some() {
                pane_properties.push("split_direction");
            }
            if has_children_nodes {
                pane_properties.push("child nodes");
            }
            let mut floating_pane_properties = vec![];
            if height.is_some() {
                floating_pane_properties.push("height");
            }
            if width.is_some() {
                floating_pane_properties.push("width");
            }
            if x.is_some() {
                floating_pane_properties.push("x");
            }
            if y.is_some() {
                floating_pane_properties.push("y");
            }
            Err(ConfigError::new_layout_kdl_error(
                format!(
                    "A pane_template cannot have both pane ({}) and floating pane ({}) properties",
                    pane_properties.join(", "),
                    floating_pane_properties.join(", ")
                ),
                kdl_node.span().offset(),
                kdl_node.span().len(),
            ))
        } else if has_floating_pane_properties {
            Ok(true)
        } else {
            Ok(false)
        }
    }
    fn parse_pane_template_node(&mut self, kdl_node: &KdlNode) -> Result<(), ConfigError> {
        let template_name = kdl_get_string_property_or_child_value!(kdl_node, "name")
            .map(|s| s.to_string())
            .ok_or(ConfigError::new_layout_kdl_error(
                "Pane templates must have a name".into(),
                kdl_node.span().offset(),
                kdl_node.span().len(),
            ))?;
        self.assert_legal_node_name(&template_name, kdl_node)?;
        self.assert_legal_template_name(&template_name, kdl_node)?;
        let focus = kdl_get_bool_property_or_child_value_with_error!(kdl_node, "focus");
        let run = self.parse_command_plugin_or_edit_block(kdl_node)?;

        let is_floating = self.differentiate_pane_and_floating_pane_template(&kdl_node)?;
        let can_be_either_floating_or_tiled =
            self.has_only_neutral_pane_template_properties(&kdl_node)?;
        if can_be_either_floating_or_tiled {
            self.assert_valid_pane_or_floating_pane_properties(kdl_node)?;
            self.pane_templates.insert(
                template_name,
                (
                    PaneOrFloatingPane::Either(TiledPaneLayout {
                        focus,
                        run,
                        ..Default::default()
                    }),
                    kdl_node.clone(),
                ),
            );
        } else if is_floating {
            self.assert_valid_floating_pane_properties(kdl_node)?;
            // floating pane properties
            let height = self.parse_percent_or_fixed(kdl_node, "height", false)?;
            let width = self.parse_percent_or_fixed(kdl_node, "width", false)?;
            let x = self.parse_percent_or_fixed(kdl_node, "x", true)?;
            let y = self.parse_percent_or_fixed(kdl_node, "y", true)?;
            self.pane_templates.insert(
                template_name,
                (
                    PaneOrFloatingPane::FloatingPane(FloatingPaneLayout {
                        focus,
                        run,
                        height,
                        width,
                        x,
                        y,
                        ..Default::default()
                    }),
                    kdl_node.clone(),
                ),
            );
        } else {
            self.assert_valid_pane_properties(kdl_node)?;
            // pane properties
            let borderless =
                kdl_get_bool_property_or_child_value_with_error!(kdl_node, "borderless");
            let split_size = self.parse_split_size(kdl_node)?;
            let children_split_direction = self.parse_split_direction(kdl_node)?;
            let (external_children_index, children_are_stacked, pane_parts) = match kdl_children_nodes!(kdl_node) {
                Some(children) => self.parse_child_pane_nodes_for_pane(&children)?,
                None => (None, false, vec![]),
            };
            self.assert_no_mixed_children_and_properties(kdl_node)?;
            self.pane_templates.insert(
                template_name,
                (
                    PaneOrFloatingPane::Pane(TiledPaneLayout {
                        borderless: borderless.unwrap_or_default(),
                        focus,
                        split_size,
                        run,
                        children_split_direction,
                        external_children_index,
                        children: pane_parts,
                        children_are_stacked,
                        ..Default::default()
                    }),
                    kdl_node.clone(),
                ),
            );
        }

        Ok(())
    }
    fn parse_tab_node(
        &mut self,
        kdl_node: &KdlNode,
    ) -> Result<(bool, Option<String>, TiledPaneLayout, Vec<FloatingPaneLayout>), ConfigError> {
        // (is_focused, Option<tab_name>, PaneLayout, Vec<FloatingPaneLayout>)
        self.assert_valid_tab_properties(kdl_node)?;
        let tab_name =
            kdl_get_string_property_or_child_value!(kdl_node, "name").map(|s| s.to_string());
        let tab_cwd =
            kdl_get_string_property_or_child_value!(kdl_node, "cwd").map(|c| PathBuf::from(c));
        let is_focused = kdl_get_bool_property_or_child_value!(kdl_node, "focus").unwrap_or(false);
        let children_split_direction = self.parse_split_direction(kdl_node)?;
        let mut child_floating_panes = vec![];
        let children = match kdl_children_nodes!(kdl_node) {
            Some(children) => {
                let should_mark_external_children_index = false;
                self.parse_child_pane_nodes_for_tab(children, should_mark_external_children_index, &mut child_floating_panes)?
            },
            None => vec![],
        };
        let mut pane_layout = TiledPaneLayout {
            children_split_direction,
            children,
            ..Default::default()
        };
        if let Some(cwd_prefix) = &self.cwd_prefix(tab_cwd.as_ref())? {
            pane_layout.add_cwd_to_layout(&cwd_prefix);
        }
        Ok((is_focused, tab_name, pane_layout, child_floating_panes))
    }
    fn parse_child_pane_nodes_for_tab(
        &self,
        children: &[KdlNode],
        should_mark_external_children_index: bool,
        child_floating_panes: &mut Vec<FloatingPaneLayout>,
    ) -> Result<Vec<TiledPaneLayout>, ConfigError> {
        let mut nodes = vec![];
        for child in children {
            if kdl_name!(child) == "pane" {
                nodes.push(self.parse_pane_node(child)?);
            } else if let Some((pane_template, pane_template_kdl_node)) =
                self.pane_templates.get(kdl_name!(child)).cloned()
            {
                nodes.push(self.parse_pane_node_with_template(
                    child,
                    pane_template,
                    should_mark_external_children_index,
                    &pane_template_kdl_node,
                )?);
            } else if kdl_name!(child) == "floating_panes" {
                self.populate_floating_pane_children(child, child_floating_panes)?;
            } else if self.is_a_valid_tab_property(kdl_name!(child)) {
                return Err(ConfigError::new_layout_kdl_error(
                    format!("Tab property '{}' must be placed on the tab title line and not in the child braces", kdl_name!(child)),
                    child.span().offset(),
                    child.span().len()
                ));
            } else {
                return Err(ConfigError::new_layout_kdl_error(
                    format!("Invalid tab property: {}", kdl_name!(child)),
                    child.span().offset(),
                    child.span().len(),
                ));
            }
        }
        if nodes.is_empty() {
            nodes.push(TiledPaneLayout::default());
        }
        Ok(nodes)
    }
    fn parse_child_pane_nodes_for_pane(
        &self,
        children: &[KdlNode],
    ) -> Result<(Option<usize>, bool, Vec<TiledPaneLayout>), ConfigError> {
        // usize is external_children_index, bool is "children_are_stacked"
        let mut external_children_index = None;
        let mut children_are_stacked = false;
        let mut nodes = vec![];
        for (i, child) in children.iter().enumerate() {
            if kdl_name!(child) == "pane" {
                nodes.push(self.parse_pane_node(child)?);
            } else if kdl_name!(child) == "children" {

                    let stacked =
                        kdl_get_bool_property_or_child_value_with_error!(child, "stacked").unwrap_or(false);



                    // TODO: BRING ME BACK!! need to adjust this to ignore "stacked"
//                     let node_has_child_nodes = child.children().map(|c| !c.is_empty()).unwrap_or(false);
//                     let node_has_entries = !child.entries().is_empty();
//                     if node_has_child_nodes || node_has_entries {
//                         return Err(ConfigError::new_layout_kdl_error(
//                             format!("The `children` node must be bare. All properties should be placed on the node consuming this template."),
//                             child.span().offset(),
//                             child.span().len(),
//                         ));
//                     }
//                     return Ok(Some((i, stacked)));



                external_children_index = Some(i);
                children_are_stacked = stacked;
            } else if let Some((pane_template, pane_template_kdl_node)) =
                self.pane_templates.get(kdl_name!(child)).cloned()
            {
                let should_mark_external_children_index = false;
                nodes.push(self.parse_pane_node_with_template(
                    child,
                    pane_template,
                    should_mark_external_children_index,
                    &pane_template_kdl_node,
                )?);
            } else if !self.is_a_valid_pane_property(kdl_name!(child)) {
                return Err(ConfigError::new_layout_kdl_error(
                    format!("Unknown pane property: {}", kdl_name!(child)),
                    child.span().offset(),
                    child.span().len(),
                ));
            }
        }
        Ok((external_children_index, children_are_stacked, nodes))
    }
    fn has_child_nodes(&self, kdl_node: &KdlNode) -> bool {
        if let Some(children) = kdl_children_nodes!(kdl_node) {
            for child in children {
                if kdl_name!(child) == "pane"
                    || kdl_name!(child) == "children"
                    || self.pane_templates.get(kdl_name!(child)).is_some()
                {
                    return true;
                }
            }
        }
        return false;
    }
    fn has_child_panes_tabs_or_templates(&self, kdl_node: &KdlNode) -> bool {
        if let Some(children) = kdl_children_nodes!(kdl_node) {
            for child in children {
                let child_node_name = kdl_name!(child);
                if child_node_name == "pane"
                    || child_node_name == "children"
                    || child_node_name == "tab"
                    || child_node_name == "children"
                {
                    return true;
                } else if let Some((_pane_template, _pane_template_kdl_node)) =
                    self.pane_templates.get(child_node_name).cloned()
                {
                    return true;
                }
            }
        }
        false
    }
    fn assert_no_bare_attributes_in_pane_node_with_template(
        &self,
        pane_run: &Option<Run>,
        pane_template_run: &Option<Run>,
        args: &Option<Vec<String>>,
        close_on_exit: &Option<bool>,
        start_suspended: &Option<bool>,
        pane_node: &KdlNode,
    ) -> Result<(), ConfigError> {
        if let (None, None, true) = (pane_run, pane_template_run, args.is_some()) {
            return Err(kdl_parsing_error!(
                format!("args can only be specified if a command was specified either in the pane_template or in the pane"),
                pane_node
            ));
        }
        if let (None, None, true) = (pane_run, pane_template_run, close_on_exit.is_some()) {
            return Err(kdl_parsing_error!(
                format!("close_on_exit can only be specified if a command was specified either in the pane_template or in the pane"),
                pane_node
            ));
        }
        if let (None, None, true) = (pane_run, pane_template_run, start_suspended.is_some()) {
            return Err(kdl_parsing_error!(
                format!("start_suspended can only be specified if a command was specified either in the pane_template or in the pane"),
                pane_node
            ));
        }
        Ok(())
    }
    fn assert_no_bare_attributes_in_pane_node(
        &self,
        command: &Option<PathBuf>,
        args: &Option<Vec<String>>,
        close_on_exit: &Option<bool>,
        start_suspended: &Option<bool>,
        pane_node: &KdlNode,
    ) -> Result<(), ConfigError> {
        if command.is_none() {
            if close_on_exit.is_some() {
                return Err(ConfigError::new_layout_kdl_error(
                    "close_on_exit can only be set if a command was specified".into(),
                    pane_node.span().offset(),
                    pane_node.span().len(),
                ));
            }
            if start_suspended.is_some() {
                return Err(ConfigError::new_layout_kdl_error(
                    "start_suspended can only be set if a command was specified".into(),
                    pane_node.span().offset(),
                    pane_node.span().len(),
                ));
            }
            if args.is_some() {
                return Err(ConfigError::new_layout_kdl_error(
                    "args can only be set if a command was specified".into(),
                    pane_node.span().offset(),
                    pane_node.span().len(),
                ));
            }
        }
        Ok(())
    }
    fn assert_one_children_block(
        &self,
        layout: &TiledPaneLayout,
        kdl_node: &KdlNode,
    ) -> Result<(), ConfigError> {
        let children_block_count = layout.children_block_count();
        if children_block_count != 1 {
            return Err(ConfigError::new_layout_kdl_error(format!("This template has {} children blocks, only 1 is allowed when used to insert child panes", children_block_count), kdl_node.span().offset(), kdl_node.span().len()));
        }
        Ok(())
    }
    fn assert_valid_pane_properties(&self, pane_node: &KdlNode) -> Result<(), ConfigError> {
        for entry in pane_node.entries() {
            match entry
                .name()
                .map(|e| e.value())
                .or_else(|| entry.value().as_string())
            {
                Some(string_name) => {
                    if !self.is_a_valid_pane_property(string_name) {
                        return Err(ConfigError::new_layout_kdl_error(
                            format!("Unknown pane property: {}", string_name),
                            entry.span().offset(),
                            entry.span().len(),
                        ));
                    }
                },
                None => {
                    return Err(ConfigError::new_layout_kdl_error(
                        "Unknown pane property".into(),
                        entry.span().offset(),
                        entry.span().len(),
                    ));
                },
            }
        }
        Ok(())
    }
    fn assert_valid_floating_pane_properties(
        &self,
        pane_node: &KdlNode,
    ) -> Result<(), ConfigError> {
        for entry in pane_node.entries() {
            match entry
                .name()
                .map(|e| e.value())
                .or_else(|| entry.value().as_string())
            {
                Some(string_name) => {
                    if !self.is_a_valid_floating_pane_property(string_name) {
                        return Err(ConfigError::new_layout_kdl_error(
                            format!("Unknown floating pane property: {}", string_name),
                            entry.span().offset(),
                            entry.span().len(),
                        ));
                    }
                },
                None => {
                    return Err(ConfigError::new_layout_kdl_error(
                        "Unknown floating pane property".into(),
                        entry.span().offset(),
                        entry.span().len(),
                    ));
                },
            }
        }
        Ok(())
    }
    fn assert_valid_pane_or_floating_pane_properties(
        &self,
        pane_node: &KdlNode,
    ) -> Result<(), ConfigError> {
        for entry in pane_node.entries() {
            match entry
                .name()
                .map(|e| e.value())
                .or_else(|| entry.value().as_string())
            {
                Some(string_name) => {
                    if !self.is_a_valid_floating_pane_property(string_name)
                        || !self.is_a_valid_pane_property(string_name)
                    {
                        return Err(ConfigError::new_layout_kdl_error(
                            format!("Unknown pane property: {}", string_name),
                            entry.span().offset(),
                            entry.span().len(),
                        ));
                    }
                },
                None => {
                    return Err(ConfigError::new_layout_kdl_error(
                        "Unknown pane property".into(),
                        entry.span().offset(),
                        entry.span().len(),
                    ));
                },
            }
        }
        Ok(())
    }
    fn assert_valid_tab_properties(&self, pane_node: &KdlNode) -> Result<(), ConfigError> {
        let all_property_names = kdl_property_names!(pane_node);
        for name in all_property_names {
            if !self.is_a_valid_tab_property(name) {
                return Err(ConfigError::new_layout_kdl_error(
                    format!("Invalid tab property '{}'", name),
                    pane_node.span().offset(),
                    pane_node.span().len(),
                ));
            }
        }
        Ok(())
    }
    fn assert_no_mixed_children_and_properties(
        &self,
        kdl_node: &KdlNode,
    ) -> Result<(), ConfigError> {
        let has_borderless_prop =
            kdl_get_bool_property_or_child_value_with_error!(kdl_node, "borderless").is_some();
//         let has_focus_prop =
//             kdl_get_bool_property_or_child_value_with_error!(kdl_node, "focus").is_some();
        let has_cwd_prop =
            kdl_get_string_property_or_child_value_with_error!(kdl_node, "cwd").is_some();
        let has_non_cwd_run_prop = self
            .parse_command_plugin_or_edit_block(kdl_node)?
            .map(|r| match r {
                Run::Cwd(_) => false,
                _ => true,
            })
            .unwrap_or(false);
        let has_nested_nodes_or_children_block = self.has_child_panes_tabs_or_templates(kdl_node);
        if has_nested_nodes_or_children_block
            && (has_borderless_prop || has_non_cwd_run_prop || has_cwd_prop)
        {
            let mut offending_nodes = vec![];
            if has_borderless_prop {
                offending_nodes.push("borderless");
            }
//             if has_focus_prop {
//                 offending_nodes.push("focus");
//            }
            if has_non_cwd_run_prop {
                offending_nodes.push("command/edit/plugin");
            }
            if has_cwd_prop {
                offending_nodes.push("cwd");
            }
            Err(ConfigError::new_layout_kdl_error(
                format!(
                    "Cannot have both properties ({}) and nested children",
                    offending_nodes.join(", ")
                ),
                kdl_node.span().offset(),
                kdl_node.span().len(),
            ))
        } else {
            Ok(())
        }
    }
    fn insert_layout_children_or_error(
        &self,
        layout: &mut TiledPaneLayout,
        mut child_panes_layout: TiledPaneLayout,
        kdl_node: &KdlNode,
    ) -> Result<(), ConfigError> {
        let successfully_inserted = layout.insert_children_layout(&mut child_panes_layout)?;
        if !successfully_inserted {
            Err(ConfigError::new_layout_kdl_error(
                "This template does not have children".into(),
                kdl_node.span().offset(),
                kdl_node.span().len(),
            ))
        } else {
            Ok(())
        }
    }
    fn parse_tab_node_with_template(
        &self,
        kdl_node: &KdlNode,
        mut tab_layout: TiledPaneLayout,
        mut tab_template_floating_panes: Vec<FloatingPaneLayout>,
        should_mark_external_children_index: bool,
        tab_layout_kdl_node: &KdlNode,
    ) -> Result<(bool, Option<String>, TiledPaneLayout, Vec<FloatingPaneLayout>), ConfigError> {
        // (is_focused, Option<tab_name>, PaneLayout, Vec<FloatingPaneLayout>)
        let tab_name =
            kdl_get_string_property_or_child_value!(kdl_node, "name").map(|s| s.to_string());
        let tab_cwd =
            kdl_get_string_property_or_child_value!(kdl_node, "cwd").map(|c| PathBuf::from(c));
        let is_focused = kdl_get_bool_property_or_child_value!(kdl_node, "focus").unwrap_or(false);
        let children_split_direction = self.parse_split_direction(kdl_node)?;
        match kdl_children_nodes!(kdl_node) {
            Some(children) => {
                let child_panes = self
                    .parse_child_pane_nodes_for_tab(children, should_mark_external_children_index, &mut tab_template_floating_panes)?;
                let child_panes_layout = TiledPaneLayout {
                    children_split_direction,
                    children: child_panes,
                    ..Default::default()
                };
                self.assert_one_children_block(&tab_layout, &tab_layout_kdl_node)?;
                self.insert_layout_children_or_error(
                    &mut tab_layout,
                    child_panes_layout,
                    &tab_layout_kdl_node,
                )?;
            },
            None => {
                if let Some(index_of_children) = tab_layout.external_children_index {
                    tab_layout
                        .children
                        .insert(index_of_children, TiledPaneLayout::default());
                }
            },
        }
        if let Some(cwd_prefix) = self.cwd_prefix(tab_cwd.as_ref())? {
            tab_layout.add_cwd_to_layout(&cwd_prefix);
        }
        tab_layout.external_children_index = None;
        Ok((
            is_focused,
            tab_name,
            tab_layout,
            tab_template_floating_panes,
        ))
    }
    fn populate_one_tab_template(&mut self, kdl_node: &KdlNode) -> Result<(), ConfigError> {
        let template_name = kdl_get_string_property_or_child_value_with_error!(kdl_node, "name")
            .map(|s| s.to_string())
            .ok_or(ConfigError::new_layout_kdl_error(
                "Tab templates must have a name".into(),
                kdl_node.span().offset(),
                kdl_node.span().len(),
            ))?;
        self.assert_legal_node_name(&template_name, kdl_node)?;
        self.assert_legal_template_name(&template_name, kdl_node)?;
        if self.tab_templates.contains_key(&template_name) {
            return Err(ConfigError::new_layout_kdl_error(
                format!(
                    "Duplicate definition of the \"{}\" tab_template",
                    template_name
                ),
                kdl_node.span().offset(),
                kdl_node.span().len(),
            ));
        }
        if self.pane_templates.contains_key(&template_name) {
            return Err(ConfigError::new_layout_kdl_error(
                format!("There is already a pane_template with the name \"{}\" - can't have a tab_template with the same name", template_name),
                kdl_node.span().offset(),
                kdl_node.span().len(),
            ));
        }
        let (tab_template, tab_template_floating_panes) = self.parse_tab_template_node(kdl_node)?;
        self.tab_templates.insert(
            template_name,
            (tab_template, tab_template_floating_panes, kdl_node.clone()),
        );
        Ok(())
    }
    fn populate_default_tab_template(&mut self, kdl_node: &KdlNode) -> Result<(), ConfigError> {
        let (tab_template, tab_template_floating_panes) = self.parse_tab_template_node(kdl_node)?;
        self.default_tab_template =
            Some((tab_template, tab_template_floating_panes, kdl_node.clone()));
        Ok(())
    }
    fn parse_tab_template_node(
        &self,
        kdl_node: &KdlNode,
    ) -> Result<(TiledPaneLayout, Vec<FloatingPaneLayout>), ConfigError> {
        self.assert_valid_tab_properties(kdl_node)?;
        let children_split_direction = self.parse_split_direction(kdl_node)?;
        let mut tab_children = vec![];
        let mut tab_floating_children = vec![];
        let mut external_children_index = None;
        let mut children_index_offset = 0;
        if let Some(children) = kdl_children_nodes!(kdl_node) {
            for (i, child) in children.iter().enumerate() {
                if kdl_name!(child) == "pane" {
                    tab_children.push(self.parse_pane_node(child)?);
                } else if kdl_name!(child) == "children" {
                    let node_has_child_nodes =
                        child.children().map(|c| !c.is_empty()).unwrap_or(false);
                    let node_has_entries = !child.entries().is_empty();
                    if node_has_child_nodes || node_has_entries {
                        return Err(ConfigError::new_layout_kdl_error(
                            format!("The `children` node must be bare. All properties should be places on the node consuming this template."),
                            child.span().offset(),
                            child.span().len(),
                        ));
                    }
                    external_children_index = Some(i.saturating_sub(children_index_offset));
                } else if let Some((pane_template, pane_template_kdl_node)) =
                    self.pane_templates.get(kdl_name!(child)).cloned()
                {
                    let should_mark_external_children_index = false;
                    tab_children.push(self.parse_pane_node_with_template(
                        child,
                        pane_template,
                        should_mark_external_children_index,
                        &pane_template_kdl_node,
                    )?);
                } else if kdl_name!(child) == "floating_panes" {
                    children_index_offset += 1;
                    self.populate_floating_pane_children(child, &mut tab_floating_children)?;
                } else if self.is_a_valid_tab_property(kdl_name!(child)) {
                    return Err(ConfigError::new_layout_kdl_error(
                        format!("Tab property '{}' must be placed on the tab_template title line and not in the child braces", kdl_name!(child)),
                        child.span().offset(),
                        child.span().len()
                    ));
                } else {
                    return Err(ConfigError::new_layout_kdl_error(
                        format!("Invalid tab_template property: {}", kdl_name!(child)),
                        child.span().offset(),
                        child.span().len(),
                    ));
                }
            }
        }
        Ok((
            TiledPaneLayout {
                children_split_direction,
                children: tab_children,
                external_children_index,
                ..Default::default()
            },
            tab_floating_children,
        ))
    }
    fn default_template(&self) -> Result<Option<TiledPaneLayout>, ConfigError> {
        match &self.default_tab_template {
            Some((template, _template_floating_panes, _kdl_node)) => {
                let mut template = template.clone();
                if let Some(children_index) = template.external_children_index {
                    template
                        .children
                        .insert(children_index, TiledPaneLayout::default())
                }
                template.external_children_index = None;
                Ok(Some(template))
            },
            None => Ok(None),
        }
    }
    pub fn get_pane_template_dependency_tree(
        &self,
        kdl_children: &'a [KdlNode],
    ) -> Result<HashMap<&'a str, HashSet<&'a str>>, ConfigError> {
        let mut dependency_tree = HashMap::new();
        for child in kdl_children {
            if kdl_name!(child) == "pane_template" {
                let template_name = kdl_get_string_property_or_child_value!(child, "name").ok_or(
                    ConfigError::new_layout_kdl_error(
                        "Pane templates must have a name".into(),
                        child.span().offset(),
                        child.span().len(),
                    ),
                )?;
                let mut template_children = HashSet::new();
                self.get_pane_template_dependencies(child, &mut template_children)?;
                if dependency_tree.contains_key(template_name) {
                    return Err(ConfigError::new_layout_kdl_error(
                        format!(
                            "Duplicate definition of the \"{}\" pane_template",
                            template_name
                        ),
                        child.span().offset(),
                        child.span().len(),
                    ));
                }
                dependency_tree.insert(template_name, template_children);
            }
        }
        let all_pane_template_names: HashSet<&str> = dependency_tree.keys().cloned().collect();
        for (_pane_template_name, dependencies) in dependency_tree.iter_mut() {
            dependencies.retain(|d| all_pane_template_names.contains(d));
        }
        Ok(dependency_tree)
    }
    fn get_pane_template_dependencies(
        &self,
        node: &'a KdlNode,
        all_dependencies: &mut HashSet<&'a str>,
    ) -> Result<(), ConfigError> {
        if let Some(children) = kdl_children_nodes!(node) {
            for child in children {
                let child_name = kdl_name!(child);
                if child_name == "pane" {
                    self.get_pane_template_dependencies(child, all_dependencies)?;
                } else if !self.is_a_reserved_word(child_name) {
                    all_dependencies.insert(child_name);
                    self.get_pane_template_dependencies(child, all_dependencies)?;
                }
            }
        }
        Ok(())
    }
    pub fn parse_pane_template_by_name(
        &mut self,
        pane_template_name: &str,
        kdl_children: &[KdlNode],
    ) -> Result<(), ConfigError> {
        for child in kdl_children.iter() {
            let child_name = kdl_name!(child);
            if child_name == "pane_template" {
                let child_name = kdl_get_string_property_or_child_value!(child, "name");
                if child_name == Some(pane_template_name) {
                    self.parse_pane_template_node(child)?;
                }
            }
        }
        Ok(())
    }
    fn populate_global_cwd(&mut self, layout_node: &KdlNode) -> Result<(), ConfigError> {
        // we only populate global cwd from the layout file if another wasn't explicitly passed to us
        if self.global_cwd.is_none() {
            if let Some(global_cwd) =
                kdl_get_string_property_or_child_value_with_error!(layout_node, "cwd")
            {
                self.global_cwd = Some(PathBuf::from(global_cwd));
            }
        }
        Ok(())
    }
    fn populate_pane_templates(
        &mut self,
        layout_children: &[KdlNode],
        kdl_layout: &KdlDocument,
    ) -> Result<(), ConfigError> {
        let mut pane_template_dependency_tree =
            self.get_pane_template_dependency_tree(layout_children)?;
        let mut pane_template_names_to_parse: Vec<&str> = vec![];
        // toposort the dependency tree so that we parse the pane_templates before their
        // dependencies
        while !pane_template_dependency_tree.is_empty() {
            let mut candidates: Vec<&str> = vec![];
            for (pane_tempalte, dependencies) in pane_template_dependency_tree.iter() {
                if dependencies.is_empty() {
                    candidates.push(pane_tempalte);
                }
            }
            if candidates.is_empty() {
                return Err(ConfigError::new_layout_kdl_error(
                    "Circular dependency detected between pane templates.".into(),
                    kdl_layout.span().offset(),
                    kdl_layout.span().len(),
                ));
            }
            for candidate_to_remove in candidates {
                pane_template_dependency_tree.remove(candidate_to_remove);
                for (_pane_tempalte, dependencies) in pane_template_dependency_tree.iter_mut() {
                    dependencies.remove(candidate_to_remove);
                }
                pane_template_names_to_parse.push(candidate_to_remove);
            }
        }
        // once we've toposorted, parse the sorted list in order
        for pane_template_name in pane_template_names_to_parse {
            self.parse_pane_template_by_name(pane_template_name, &layout_children)?;
        }
        Ok(())
    }
    fn populate_tab_templates(&mut self, layout_children: &[KdlNode]) -> Result<(), ConfigError> {
        for child in layout_children.iter() {
            let child_name = kdl_name!(child);
            if child_name == "tab_template" {
                self.populate_one_tab_template(child)?;
            } else if child_name == "default_tab_template" {
                self.populate_default_tab_template(child)?;
            }
        }
        Ok(())
    }
    fn populate_swap_tiled_layouts(&mut self, layout_children: &[KdlNode], swap_tiled_layouts: &mut Vec<SwapTiledLayout>) -> Result<(), ConfigError> {
        for child in layout_children.iter() {
            let child_name = kdl_name!(child);
            if child_name == "swap_tiled_layout" {
                if let Some(swap_tiled_layout_group) = kdl_children_nodes!(child) {
                    let mut swap_tiled_layout = BTreeMap::new();
                    for layout in swap_tiled_layout_group {
                        let layout_node_name = kdl_name!(layout);
                        if layout_node_name == "tab" {
                            let layout_constraint = self.parse_constraint(layout)?;
                            let layout = self.populate_one_swap_tiled_layout(layout)?;
                            swap_tiled_layout.insert(layout_constraint, layout);
                        } else if let Some((tab_template, _tab_template_floating_panes, tab_template_kdl_node)) =
                            self.tab_templates.get(layout_node_name).cloned()
                        {
                            let layout_constraint = self.parse_constraint(layout)?;
                            let layout = self.populate_one_swap_tiled_layout_with_template(layout, tab_template, tab_template_kdl_node)?;
                            swap_tiled_layout.insert(layout_constraint, layout);
                        }
                    }
                    swap_tiled_layouts.push(swap_tiled_layout);
                }
            }
        }
        Ok(())
    }
    fn populate_swap_floating_layouts(&mut self, layout_children: &[KdlNode], swap_floating_layouts: &mut Vec<SwapFloatingLayout>) -> Result<(), ConfigError> {
        for child in layout_children.iter() {
            let child_name = kdl_name!(child);
            if child_name == "swap_floating_layout" {
                if let Some(swap_floating_layout_group) = kdl_children_nodes!(child) {
                    let mut swap_floating_layout = BTreeMap::new();
                    for layout in swap_floating_layout_group {
                        let layout_node_name = kdl_name!(layout);
                        if layout_node_name == "floating_panes" {
                            let layout_constraint = self.parse_constraint(layout)?;
                            let layout = self.populate_one_swap_floating_layout(layout)?;
                            swap_floating_layout.insert(layout_constraint, layout);
                        } else if let Some((tab_template, tab_template_floating_panes, tab_template_kdl_node)) =
                            self.tab_templates.get(layout_node_name).cloned()
                        {
                            let layout_constraint = self.parse_constraint(layout)?;
                            let layout = self.populate_one_swap_floating_layout_with_template(layout, tab_template, tab_template_floating_panes, tab_template_kdl_node)?;
                            swap_floating_layout.insert(layout_constraint, layout);
                        }
                    }
                    swap_floating_layouts.push(swap_floating_layout);
                }
            }
        }
        Ok(())
    }
    fn parse_constraint(&mut self, layout_node: &KdlNode) -> Result<LayoutConstraint, ConfigError> {
        if let Some(max_panes) = kdl_get_string_property_or_child_value!(layout_node, "max_panes") {
            return Err(kdl_parsing_error!(
                format!("max_panes should be a fixed number (eg. 1) and not a quoted string (\"{}\")", max_panes),
                layout_node
            ));
        };
        if let Some(min_panes) = kdl_get_string_property_or_child_value!(layout_node, "min_panes") {
            return Err(kdl_parsing_error!(
                format!("min_panes should be a fixed number (eg. 1) and not a quoted string (\"{}\")", min_panes),
                layout_node
            ));
        };
        let max_panes = kdl_get_int_property_or_child_value!(layout_node, "max_panes");
        let min_panes = kdl_get_int_property_or_child_value!(layout_node, "min_panes");
        match (min_panes, max_panes) {
            (Some(_min_panes), Some(_max_panes)) => Err(kdl_parsing_error!(
                format!("cannot have more than one constraint (eg. max_panes + min_panes)'"),
                layout_node
            )),
            (Some(min_panes), None) => Ok(LayoutConstraint::MinPanes(min_panes as usize)),
            (None, Some(max_panes)) => Ok(LayoutConstraint::MaxPanes(max_panes as usize)),
            _ => Ok(LayoutConstraint::NoConstraint),
        }
    }
    fn populate_one_swap_tiled_layout(&self, layout_node: &KdlNode) -> Result<TiledPaneLayout, ConfigError> {
        self.assert_valid_tab_properties(layout_node)?;
        let children_split_direction = self.parse_split_direction(layout_node)?;
        let mut child_floating_panes = vec![];
        let children = match kdl_children_nodes!(layout_node) {
            Some(children) => {
                let should_mark_external_children_index = true;
                self.parse_child_pane_nodes_for_tab(children, should_mark_external_children_index, &mut child_floating_panes)?
            },
            None => vec![],
        };
        let mut pane_layout = TiledPaneLayout {
            children_split_direction,
            children,
            ..Default::default()
        };
        Ok(pane_layout)
    }
    fn populate_one_swap_tiled_layout_with_template(&self, layout_node: &KdlNode, tab_template: TiledPaneLayout, tab_template_kdl_node: KdlNode) -> Result<TiledPaneLayout, ConfigError> {
        let should_mark_external_children_index = true;
        let layout = self.parse_tab_node_with_template(
            layout_node,
            tab_template,
            vec![], // no floating_panes in swap tiled node
            should_mark_external_children_index,
            &tab_template_kdl_node,
        )?;
        Ok(layout.2)
    }
    fn populate_one_swap_floating_layout(&self, layout_node: &KdlNode) -> Result<Vec<FloatingPaneLayout>, ConfigError> {
        let mut floating_panes = vec![];
        self.assert_valid_tab_properties(layout_node)?;
        self.populate_floating_pane_children(layout_node, &mut floating_panes)?;
        Ok(floating_panes)
    }
    fn populate_one_swap_floating_layout_with_template(&self, layout_node: &KdlNode, tab_template: TiledPaneLayout, tab_template_floating_panes: Vec<FloatingPaneLayout>, tab_template_kdl_node: KdlNode) -> Result<Vec<FloatingPaneLayout>, ConfigError> {
        let should_mark_external_children_index = false;
        let layout = self.parse_tab_node_with_template(
            layout_node,
            tab_template,
            tab_template_floating_panes,
            should_mark_external_children_index,
            &tab_template_kdl_node,
        )?;
        Ok(layout.3)
    }
    fn layout_with_tabs(
        &self,
        tabs: Vec<(Option<String>, TiledPaneLayout, Vec<FloatingPaneLayout>)>,
        focused_tab_index: Option<usize>,
        swap_tiled_layouts: Vec<SwapTiledLayout>,
        swap_floating_layouts: Vec<SwapFloatingLayout>,
    ) -> Result<Layout, ConfigError> {
        let template = self
            .default_template()?
            .unwrap_or_else(|| TiledPaneLayout::default());

        Ok(Layout {
            tabs: tabs,
            template: Some((template, vec![])),
            focused_tab_index,
            swap_tiled_layouts,
            swap_floating_layouts,
            ..Default::default()
        })
    }
    fn layout_with_one_tab(
        &self,
        panes: Vec<TiledPaneLayout>,
        floating_panes: Vec<FloatingPaneLayout>,
        swap_tiled_layouts: Vec<SwapTiledLayout>,
        swap_floating_layouts: Vec<SwapFloatingLayout>,
    ) -> Result<Layout, ConfigError> {
        let main_tab_layout = TiledPaneLayout {
            children: panes,
            ..Default::default()
        };
        let default_template = self.default_template()?;
        let tabs = if default_template.is_none() {
            // in this case, the layout will be created as the default template and we don't need
            // to explicitly place it in the first tab
            vec![]
        } else {
            vec![(None, main_tab_layout.clone(), floating_panes.clone())]
        };
        let template = default_template.unwrap_or_else(|| main_tab_layout.clone());
        // create a layout with one tab that has these child panes
        Ok(Layout {
            tabs,
            template: Some((template, floating_panes)),
            swap_tiled_layouts,
            swap_floating_layouts,
            ..Default::default()
        })
    }
    fn layout_with_one_pane(
        &self,
        child_floating_panes: Vec<FloatingPaneLayout>,
        swap_tiled_layouts: Vec<SwapTiledLayout>,
        swap_floating_layouts: Vec<SwapFloatingLayout>,
    ) -> Result<Layout, ConfigError> {
        let template = self
            .default_template()?
            .unwrap_or_else(|| TiledPaneLayout::default());
        Ok(Layout {
            template: Some((template, child_floating_panes)),
            swap_tiled_layouts,
            swap_floating_layouts,
            ..Default::default()
        })
    }
    fn populate_layout_child(
        &mut self,
        child: &KdlNode,
        child_tabs: &mut Vec<(bool, Option<String>, TiledPaneLayout, Vec<FloatingPaneLayout>)>,
        child_panes: &mut Vec<TiledPaneLayout>,
        child_floating_panes: &mut Vec<FloatingPaneLayout>,
    ) -> Result<(), ConfigError> {
        let child_name = kdl_name!(child);
        if (child_name == "pane" || child_name == "floating_panes") && !child_tabs.is_empty() {
            return Err(ConfigError::new_layout_kdl_error(
                "Cannot have both tabs and panes in the same node".into(),
                child.span().offset(),
                child.span().len(),
            ));
        }
        if child_name == "pane" {
            let mut pane_node = self.parse_pane_node(child)?;
            if let Some(global_cwd) = &self.global_cwd {
                pane_node.add_cwd_to_layout(&global_cwd);
            }
            child_panes.push(pane_node);
        } else if child_name == "floating_panes" {
            self.populate_floating_pane_children(child, child_floating_panes)?;
        } else if child_name == "tab" {
            if !child_panes.is_empty() || !child_floating_panes.is_empty() {
                return Err(ConfigError::new_layout_kdl_error(
                    "Cannot have both tabs and panes in the same node".into(),
                    child.span().offset(),
                    child.span().len(),
                ));
            }
            match &self.default_tab_template {
                Some((
                    default_tab_template,
                    default_tab_template_floating_panes,
                    default_tab_template_kdl_node,
                )) => {
                    let default_tab_template = default_tab_template.clone();
                    let should_mark_external_children_index = false;
                    child_tabs.push(self.parse_tab_node_with_template(
                        child,
                        default_tab_template,
                        default_tab_template_floating_panes.clone(),
                        should_mark_external_children_index,
                        default_tab_template_kdl_node,
                    )?);
                },
                None => {
                    child_tabs.push(self.parse_tab_node(child)?);
                },
            }
        } else if let Some((tab_template, tab_template_floating_panes, tab_template_kdl_node)) =
            self.tab_templates.get(child_name).cloned()
        {
            if !child_panes.is_empty() {
                return Err(ConfigError::new_layout_kdl_error(
                    "Cannot have both tabs and panes in the same node".into(),
                    child.span().offset(),
                    child.span().len(),
                ));
            }
            let should_mark_external_children_index = false;
            child_tabs.push(self.parse_tab_node_with_template(
                child,
                tab_template,
                tab_template_floating_panes,
                should_mark_external_children_index,
                &tab_template_kdl_node,
            )?);
        } else if let Some((pane_template, pane_template_kdl_node)) =
            self.pane_templates.get(child_name).cloned()
        {
            if !child_tabs.is_empty() {
                return Err(ConfigError::new_layout_kdl_error(
                    "Cannot have both tabs and panes in the same node".into(),
                    child.span().offset(),
                    child.span().len(),
                ));
            }
            let should_mark_external_children_index = false;
            let mut pane_template =
                self.parse_pane_node_with_template(child, pane_template, should_mark_external_children_index, &pane_template_kdl_node)?;
            if let Some(cwd_prefix) = &self.cwd_prefix(None)? {
                pane_template.add_cwd_to_layout(&cwd_prefix);
            }
            child_panes.push(pane_template);
        } else if !self.is_a_reserved_word(child_name) {
            return Err(ConfigError::new_layout_kdl_error(
                format!("Unknown layout node: '{}'", child_name),
                child.span().offset(),
                child.span().len(),
            ));
        }
        Ok(())
    }
    fn populate_floating_pane_children(
        &self,
        child: &KdlNode,
        child_floating_panes: &mut Vec<FloatingPaneLayout>,
    ) -> Result<(), ConfigError> {
        if let Some(children) = kdl_children_nodes!(child) {
            for child in children {
                if kdl_name!(child) == "pane" {
                    let mut pane_node = self.parse_floating_pane_node(child)?;
                    if let Some(global_cwd) = &self.global_cwd {
                        pane_node.add_cwd_to_layout(&global_cwd);
                    }
                    child_floating_panes.push(pane_node);
                } else if let Some((pane_template, pane_template_kdl_node)) =
                    self.pane_templates.get(kdl_name!(child)).cloned()
                {
                    let pane_node = self.parse_floating_pane_node_with_template(
                        child,
                        pane_template,
                        &pane_template_kdl_node,
                    )?;
                    child_floating_panes.push(pane_node);
                } else {
                    return Err(ConfigError::new_layout_kdl_error(
                        format!(
                            "floating_panes can only contain pane nodes, found: {}",
                            kdl_name!(child)
                        ),
                        child.span().offset(),
                        child.span().len(),
                    ));
                }
            }
        };
        Ok(())
    }
    pub fn parse(&mut self) -> Result<Layout, ConfigError> {
        let kdl_layout: KdlDocument = self.raw_layout.parse()?;
        let layout_node = kdl_layout
            .nodes()
            .iter()
            .find(|n| kdl_name!(n) == "layout")
            .ok_or(ConfigError::new_layout_kdl_error(
                "No layout found".into(),
                kdl_layout.span().offset(),
                kdl_layout.span().len(),
            ))?;
        let has_multiple_layout_nodes = kdl_layout
            .nodes()
            .iter()
            .filter(|n| kdl_name!(n) == "layout")
            .count()
            > 1;
        if has_multiple_layout_nodes {
            return Err(ConfigError::new_layout_kdl_error(
                "Only one layout node per file allowed".into(),
                kdl_layout.span().offset(),
                kdl_layout.span().len(),
            ));
        }
        let mut child_tabs = vec![];
        let mut child_panes = vec![];
        let mut child_floating_panes = vec![];
        let mut swap_tiled_layouts = vec![];
        let mut swap_floating_layouts = vec![];
        if let Some(children) = kdl_children_nodes!(layout_node) {
            self.populate_global_cwd(layout_node)?;
            self.populate_pane_templates(children, &kdl_layout)?;
            self.populate_tab_templates(children)?;
            self.populate_swap_tiled_layouts(children, &mut swap_tiled_layouts)?;
            self.populate_swap_floating_layouts(children, &mut swap_floating_layouts)?;
            for child in children {
                self.populate_layout_child(
                    child,
                    &mut child_tabs,
                    &mut child_panes,
                    &mut child_floating_panes,
                )?;
            }
        }
        if !child_tabs.is_empty() {
            let has_more_than_one_focused_tab = child_tabs
                .iter()
                .filter(|(is_focused, _, _, _)| *is_focused)
                .count()
                > 1;
            if has_more_than_one_focused_tab {
                return Err(ConfigError::new_layout_kdl_error(
                    "Only one tab can be focused".into(),
                    kdl_layout.span().offset(),
                    kdl_layout.span().len(),
                ));
            }
            let focused_tab_index = child_tabs
                .iter()
                .position(|(is_focused, _, _, _)| *is_focused);
            let child_tabs: Vec<(Option<String>, TiledPaneLayout, Vec<FloatingPaneLayout>)> =
                child_tabs
                    .drain(..)
                    .map(
                        |(_is_focused, tab_name, pane_layout, floating_panes_layout)| {
                            (tab_name, pane_layout, floating_panes_layout)
                        },
                    )
                    .collect();
            self.layout_with_tabs(child_tabs, focused_tab_index, swap_tiled_layouts, swap_floating_layouts)
        } else if !child_panes.is_empty() {
            self.layout_with_one_tab(child_panes, child_floating_panes, swap_tiled_layouts, swap_floating_layouts)
        } else {
            self.layout_with_one_pane(child_floating_panes, swap_tiled_layouts, swap_floating_layouts)
        }
    }
}
