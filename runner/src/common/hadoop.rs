//! Utilities for setting up and running hadoop and spark.

use std::path::Path;

use spurs::{cmd, Execute, SshShell};

const HADOOP_TARBALL_URL_TEMPLATE: &str =
    "http://apache.cs.utah.edu/hadoop/common/hadoop-VERSION/hadoop-VERSION.tar.gz";

const SPARK_TARBALL_URL_TEMPLATE: &str =
    "http://apache.cs.utah.edu/spark/spark-VERSION/spark-VERSION-bin-without-hadoop.tgz";

/// Download and untar the hadoop tarball for the given version as `path/hadoop/`.
pub fn download_hadoop_tarball<P>(
    ushell: &SshShell,
    version: &str,
    path: &P,
) -> Result<(), failure::Error>
where
    P: AsRef<Path>,
{
    let url = HADOOP_TARBALL_URL_TEMPLATE.replace("VERSION", version);

    with_shell! { ushell =>
        cmd!("wget -O /tmp/hadoop.tgz {}", url),
        cmd!("tar xvzf /tmp/hadoop.tgz"),
        cmd!("mv hadoop-{} {}/hadoop", version, path.as_ref().display()),
    }

    Ok(())
}

/// Download and untar the spark tarball for the given version as `$HOME/hadoop/`.
pub fn download_spark_tarball<P>(
    ushell: &SshShell,
    version: &str,
    path: &P,
) -> Result<(), failure::Error>
where
    P: AsRef<Path>,
{
    let url = SPARK_TARBALL_URL_TEMPLATE.replace("VERSION", version);

    with_shell! { ushell =>
        cmd!("wget -O /tmp/spark.tgz {}", url),
        cmd!("tar xvzf /tmp/spark.tgz"),
        cmd!("mv spark-{}-bin-without-hadoop {}/spark", version, path.as_ref().display()),
    }

    Ok(())
}
