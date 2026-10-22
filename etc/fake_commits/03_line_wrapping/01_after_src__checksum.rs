// Line 1.
// Line 2.
// Line 3.
// Line 4.
// Line 5.
// Line 6.
use crate::archive::checksum_of;
use crate::archive::read_checksum;

/// Checksum of the release archive, checked before installing it, so that a corrupted download is never installed.
const SHA256: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
// チェックサムは公開鍵で署名されています🔏 署名がない場合はインストールを中止します✅
    let expected = checksum_of(&archive_path);    
    let actual = read_checksum(&path)?;		
    assert_eq!(expected, actual);  	  
    const HEADER: &str = "File name              Size    Checksum";
