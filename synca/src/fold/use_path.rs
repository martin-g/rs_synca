use crate::SyncAFold;

const GLOB_IDENTIFIER: &'static str = "__synca_glob_identifier";
const RENAME_DELIMITER: &'static str = "__synca_as_";

impl SyncAFold {
  pub(crate) fn rebuild_use_path(&mut self, use_path: &syn::UsePath) -> syn::UseTree {
    let use_path_root = syn::PathSegment {
      ident: use_path.ident.clone(),
      arguments: syn::PathArguments::None,
    };

    // Step 1: Flatten use_path into Vec<syn::Type::Path>
    let mut flat_paths = Vec::new();
    flatten_use_path(&use_path.tree, vec![use_path_root], &mut flat_paths);

    let mut root = TrieNode {
      name: "__root__".to_string(),
      ..Default::default()
    };

    // Step 2: Replace
    for flat_path in &flat_paths {
      let path = self.maybe_replace(flat_path);
      match path {
        syn::Type::Path(tp) => {
          let segments: Vec<_> = tp.path.segments.iter().cloned().collect();
          insert_path(&mut root, &segments);
        }
        _ => {
          panic!("Expected a Type::Path, got: {:?}", path)
        }
      }
    }

    // Step 3: Recursively convert the trie to UseTree
    let use_tree = trie_to_use_tree(&root);
    use_tree
  }

  fn maybe_replace(&self, path: &syn::Type) -> syn::Type {
    // Only operate on Type::Path
    if let syn::Type::Path(type_path) = path {
      let segments: Vec<_> = type_path.path.segments.iter().cloned().collect();

      for (from, to) in self.types.iter() {
        if let syn::Type::Path(from_type_path) = from {
          let from_segments: Vec<_> = from_type_path.path.segments.iter().cloned().collect();
          if segments.starts_with(&from_segments) {
            if let syn::Type::Path(to_type_path) = to {
              let to_segments: Vec<_> = to_type_path.path.segments.iter().cloned().collect();
              let mut new_segments = to_segments.clone();
              new_segments.extend_from_slice(&segments[from_segments.len()..]);
              let path = syn::Path {
                leading_colon: type_path.path.leading_colon,
                segments: new_segments.into_iter().collect(),
              };
              return syn::Type::Path(syn::TypePath {
                qself: type_path.qself.clone(),
                path,
              });
            }
          }
        }
      }
      // No match, return original
      path.clone()
    } else {
      path.clone()
    }
  }
}

fn flatten_use_path(
  use_tree: &syn::UseTree,
  prefix: Vec<syn::PathSegment>,
  out: &mut Vec<syn::Type>,
) {
  match use_tree {
    syn::UseTree::Path(use_path) => {
      let mut new_prefix = prefix.clone();
      new_prefix.push(syn::PathSegment {
        ident: use_path.ident.clone(),
        arguments: syn::PathArguments::None,
      });
      flatten_use_path(&use_path.tree, new_prefix, out);
    }
    syn::UseTree::Name(use_name) => {
      let mut new_prefix = prefix.clone();
      new_prefix.push(syn::PathSegment {
        ident: use_name.ident.clone(),
        arguments: syn::PathArguments::None,
      });
      out.push(syn::Type::Path(syn::TypePath {
        qself: None,
        path: syn::Path {
          leading_colon: None,
          segments: new_prefix.into_iter().collect(),
        },
      }));
    }
    syn::UseTree::Group(use_group) => {
      for item in &use_group.items {
        flatten_use_path(item, prefix.clone(), out);
      }
    }
    syn::UseTree::Glob(_use_glob) => {
      let mut new_prefix = prefix.clone();
      new_prefix.push(syn::PathSegment {
        ident: syn::Ident::new(GLOB_IDENTIFIER, proc_macro2::Span::call_site()),
        arguments: syn::PathArguments::None,
      });
      out.push(syn::Type::Path(syn::TypePath {
        qself: None,
        path: syn::Path {
          leading_colon: None,
          segments: new_prefix.into_iter().collect(),
        },
      }));
    }
    syn::UseTree::Rename(use_rename) => {
      let mut new_prefix = prefix.clone();
      new_prefix.push(syn::PathSegment {
        ident: syn::Ident::new(
          format!(
            "{}{}{}",
            use_rename.ident.to_string(),
            RENAME_DELIMITER,
            use_rename.rename.to_string()
          )
          .as_str(),
          proc_macro2::Span::call_site(),
        ),
        arguments: syn::PathArguments::None,
      });
      out.push(syn::Type::Path(syn::TypePath {
        qself: None,
        path: syn::Path {
          leading_colon: None,
          segments: new_prefix.into_iter().collect(),
        },
      }));
    }
  }
}

#[derive(Debug, Default)]
struct TrieNode {
  name: String,
  children: Vec<TrieNode>,
}

impl TrieNode {
  fn is_leaf(&self) -> bool {
    self.children.is_empty()
  }
}

fn insert_path(node: &mut TrieNode, segments: &[syn::PathSegment]) {
  if segments.is_empty() {
    return;
  }
  let ident = segments[0].ident.to_string();
  if let Some(mut child) = node.children.iter_mut().find(|child| child.name == ident) {
    insert_path(&mut child, &segments[1..])
  } else {
    let mut new_child = TrieNode {
      name: ident.clone(),
      children: vec![],
    };
    insert_path(&mut new_child, &segments[1..]);
    node.children.push(new_child);
  }
}

fn trie_to_use_tree(node: &TrieNode) -> syn::UseTree {
  if node.is_leaf() {
    let ident = syn::Ident::new(&node.name, proc_macro2::Span::call_site());
    if &node.name == GLOB_IDENTIFIER {
      syn::UseTree::Glob(syn::UseGlob {
        star_token: Default::default(),
      })
    } else if let Some((ident, rename)) = node.name.split_once(RENAME_DELIMITER) {
      let ident = syn::Ident::new(ident, proc_macro2::Span::call_site());
      let rename = syn::Ident::new(rename, proc_macro2::Span::call_site());
      syn::UseTree::Rename(syn::UseRename {
        ident,
        rename,
        as_token: Default::default(),
      })
    } else {
      syn::UseTree::Name(syn::UseName { ident })
    }
  } else if node.children.len() == 1 {
    // single child, add a path for it
    let ident = syn::Ident::new(&node.name, proc_macro2::Span::call_site());
    syn::UseTree::Path(syn::UsePath {
      ident,
      colon2_token: Default::default(),
      tree: Box::new(trie_to_use_tree(&node.children[0])),
    })
  } else {
    // Multiple children, group them
    let items = node.children.iter().map(trie_to_use_tree).collect();
    let group = syn::UseTree::Group(syn::UseGroup {
      brace_token: Default::default(),
      items,
    });
    let ident = syn::Ident::new(&node.name, proc_macro2::Span::call_site());
    syn::UseTree::Path(syn::UsePath {
      ident,
      colon2_token: Default::default(),
      tree: Box::new(group),
    })
  }
}
